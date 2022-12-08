use std::ops::ControlFlow;
use std::time::SystemTime;

use apollo_router::layers::ServiceBuilderExt;
use apollo_router::plugin::Plugin;
use apollo_router::plugin::PluginInit;
use apollo_router::register_plugin;
use apollo_router::services::subgraph;
use apollo_router::graphql;

use aws_sigv4::http_request::sign;
use aws_sigv4::http_request::PayloadChecksumKind;
use aws_sigv4::http_request::SignableBody;
use aws_sigv4::http_request::SignableRequest;
use aws_sigv4::http_request::SigningParams;
use aws_sigv4::http_request::SigningSettings;
use aws_types::Credentials;

use schemars::JsonSchema;
use serde::Deserialize;
use tower::ServiceExt;

use tower::BoxError;
use tower::ServiceBuilder;

#[derive(Debug)]
struct AwsSign {
    #[allow(dead_code)]
    configuration: Conf,
}

#[derive(Debug, Default, Deserialize, JsonSchema)]
struct Conf {
    // Put your plugin configuration here. It will automatically be deserialized from JSON.
    // Always put some sort of config here, even if it is just a bool to say that the plugin is enabled,
    // otherwise the yaml to enable the plugin will be confusing.
    access_key_id: String,
    secret_access_key: String,
    region: String,
    service: String,
}
// This is a bare bones plugin that can be duplicated when creating your own.
#[async_trait::async_trait]
impl Plugin for AwsSign {
    type Config = Conf;

    async fn new(init: PluginInit<Self::Config>) -> Result<Self, BoxError> {
        Ok(AwsSign {
            configuration: init.config,
        })
    }

    fn subgraph_service(&self, _name: &str, service: subgraph::BoxService) -> subgraph::BoxService {
        let aws_credentials = Credentials::new(
            &self.configuration.access_key_id,
            &self.configuration.secret_access_key,
            None,
            None,
            "default",
        );

        let aws_region = self.configuration.region.clone();

        let aws_service = self.configuration.service.clone();

        ServiceBuilder::new()
            .checkpoint(move |mut request: subgraph::Request| {
                let now = SystemTime::now();

                let mut settings = SigningSettings::default();
                settings.payload_checksum_kind = PayloadChecksumKind::XAmzSha256;
    
                let mut builder = SigningParams::builder()
                    .access_key(aws_credentials.access_key_id())
                    .secret_key(aws_credentials.secret_access_key())
                    .region(aws_region.as_ref())
                    .service_name(aws_service.as_ref())
                    .time(now)
                    .settings(settings);
    
                builder.set_security_token(aws_credentials.session_token());
                let signing_params = builder.build().expect("all required fields set");
    
                let body_bytes = match serde_json::to_vec(&request.subgraph_request.body()) {
                    Ok(bytes) => bytes,
                    Err(err) => {
                        tracing::error!("Failed to serialize GraphQL body for AWS SigV4 signing. Error: {}", err);
                        return Ok(ControlFlow::Break(subgraph::Response::error_builder()
                                    .error(graphql::Error::builder().message("Failed to serialize GraphQL body for AWS SigV4 signing").build())
                                    .status_code(http::StatusCode::UNAUTHORIZED)
                                    .context(request.context)
                                    .build().unwrap()));
                    }
                };
    
                let signable_request = SignableRequest::new(
                    request.subgraph_request.method(),
                    request.subgraph_request.uri(),
                    request.subgraph_request.headers(),
                    SignableBody::Bytes(&body_bytes),
                );
    
                let (signing_instructions, _signature) = match sign(signable_request, &signing_params) {
                    Ok(output) => output,
                    Err(err) => {
                        tracing::error!("Failed to sign GraphQL request for AWS SigV4. Error: {}", err);
                        return Ok(ControlFlow::Break(subgraph::Response::error_builder()
                                    .error(graphql::Error::builder().message("Failed to sign GraphQL request for AWS SigV4").build())
                                    .status_code(http::StatusCode::UNAUTHORIZED)
                                    .context(request.context)
                                    .build().unwrap()));
                    }
                }.into_parts();
    
                signing_instructions.apply_to_request(&mut request.subgraph_request);
                Ok(ControlFlow::Continue(request))
            })
            .map_response(|response: subgraph::Response| {
                if !response.response.status().is_success() {
                    return match response.response.headers().get("x-amzn-errortype") {
                        Some(error) => {
                            return subgraph::Response::error_builder()
                                    .error(graphql::Error::builder().message(error.to_str().unwrap()).build())
                                    .status_code(http::StatusCode::UNAUTHORIZED)
                                    .context(response.context)
                                    .build()
                                    .unwrap()
                        },
                        None => {
                            tracing::error!("AWS SigV4 signing failed, no error type returned");
                            response
                        }
                    }
                }
                response
            })
            .buffered()
            .service(service)
            .boxed()
    }
}

// This macro allows us to use it in our plugin registry!
// register_plugin takes a group name, and a plugin name.
register_plugin!("aws", "signv4", AwsSign);

#[cfg(test)]
mod tests {
    use apollo_router::services::supergraph;
    use apollo_router::TestHarness;
    use tower::BoxError;
    use tower::ServiceExt;

    #[tokio::test]
    async fn basic_test() -> Result<(), BoxError> {
        let test_harness = TestHarness::builder()
            .configuration_json(serde_json::json!({
                "plugins": {
                    "aws.signv4": {
                        "access_key" : "myAWSid",
                        "secret_key" : "secret",
                        "region" : "us-east-1",
                        "enabled": true,
                    }
                }
            }))
            .unwrap()
            .build()
            .await
            .unwrap();
        let request = supergraph::Request::canned_builder().build().unwrap();
        let mut streamed_response = test_harness.oneshot(request).await?;

        let first_response = streamed_response
            .next_response()
            .await
            .expect("couldn't get primary response");

        assert!(first_response.data.is_some());

        println!("first response: {:?}", first_response);
        let next = streamed_response.next_response().await;
        println!("next response: {:?}", next);

        // You could keep calling .next_response() until it yields None if you're expexting more parts.
        assert!(next.is_none());
        Ok(())
    }
}
