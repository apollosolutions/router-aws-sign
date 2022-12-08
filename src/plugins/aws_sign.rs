use std::error::Error;
use std::ops::ControlFlow;
use std::time::SystemTime;

use apollo_router::layers::ServiceBuilderExt;
use apollo_router::plugin::Plugin;
use apollo_router::plugin::PluginInit;
use apollo_router::register_plugin;
use apollo_router::services::subgraph;

use aws_sig_auth::signer::OperationSigningConfig;
use aws_sig_auth::signer::RequestConfig;
use aws_sig_auth::signer::SigV4Signer;
use aws_smithy_http::body::SdkBody;
use aws_types::credentials::ProvideCredentials;
use aws_types::region::Region;
use aws_types::region::SigningRegion;
use aws_types::Credentials;
use aws_types::SigningService;

use schemars::JsonSchema;
// use serde::ser::Error;
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
    access_key: String,
    secret_key: String,
    region: String,
}
// This is a bare bones plugin that can be duplicated when creating your own.
#[async_trait::async_trait]
impl Plugin for AwsSign {
    type Config = Conf;

    async fn new(init: PluginInit<Self::Config>) -> Result<Self, BoxError> {
        tracing::info!("aws sign access key {}", init.config.access_key);
        tracing::info!("aws sign secret key {}", init.config.secret_key);
        Ok(AwsSign {
            configuration: init.config,
        })
    }

    // Delete this function if you are not customizing it.
    fn subgraph_service(&self, _name: &str, service: subgraph::BoxService) -> subgraph::BoxService {
        let provider = Credentials::new(
            &self.configuration.access_key,
            &self.configuration.access_key,
            None,
            None,
            "default",
        );

        let region = self.configuration.region.clone();

        async fn sign_request(
            request: &mut http::Request<SdkBody>,
            region: Region,
            credentials_provider: &impl ProvideCredentials,
        ) -> Result<(), Box<dyn Error + Send + Sync>> {
            let now = SystemTime::now();
            let signer = SigV4Signer::new();
            let request_config = RequestConfig {
                request_ts: now,
                region: &SigningRegion::from(region),
                service: &SigningService::from_static("execute-api"),
                payload_override: None,
            };
            signer.sign(
                &OperationSigningConfig::default_config(),
                &request_config,
                &credentials_provider.provide_credentials().await?,
                request,
            )?;
            Ok(())
        }

        ServiceBuilder::new()
            .checkpoint_async(move |mut request: subgraph::Request| {
                let region = region.clone();
                let provider = provider.clone();
                async move {
                    let (original_parts, original_body) = request.subgraph_request.into_parts();
                    let string_body = serde_json::to_string(&original_body)?;
                    let mut temp_request =
                        http::Request::from_parts(original_parts, SdkBody::from(string_body));
                    sign_request(&mut temp_request, Region::new(region.clone()), &provider).await?;
                    let (signed_parts, _signed_body) = temp_request.into_parts();
                    let signed_request = http::Request::from_parts(signed_parts, original_body);
                    request.subgraph_request = signed_request;

                    Ok(ControlFlow::Continue(request))
                }
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
