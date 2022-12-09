# Router Extension Template

**The code in this repository is experimental and has been provided for reference purposes only. Community feedback is welcome but this project may not be supported in the same way that repositories in the official [Apollo GraphQL GitHub organization](https://github.com/apollographql) are. If you need help you can file an issue on this repository, [contact Apollo](https://www.apollographql.com/contact-sales) to talk to an expert, or create a ticket directly in Apollo Studio.**

This generated project is set up to create a custom Apollo Router binary that may include plugins that you have written.

> Note: The Apollo Router is made available under the Elastic License v2.0 (ELv2).
> Read [our licensing page](https://www.apollographql.com/docs/resources/elastic-license-v2-faq/) for more details.

# Compile the router

To create a debug build use the following command.

```bash
cargo build
```

Your debug binary is now located in `target/debug/router`

For production, you will want to create a release build.

```bash
cargo build --release
```

Your release binary is now located in `target/release/router`

# Run the Apollo Router

1. Download the example schema

   ```bash
   curl -sSL https://supergraph.demo.starstuff.dev/ > supergraph-schema.graphql
   ```

2. Run the Apollo Router

   During development it is convenient to use `cargo run` to run the Apollo Router as it will

   ```bash
   cargo run -- --hot-reload --config router.yaml --supergraph supergraph-schema.graphql
   ```

> If you are using managed federation you can set APOLLO_KEY and APOLLO_GRAPH_REF environment variables instead of specifying the supergraph as a file.

# Usage

```yaml
plugins:
    aws.signv4:
        access_key_id: "key"
        secret_access_key: "secret"
        region: "us-east-1"
        service: "lambda"
```

## Licensing

Source code in this repository is covered by the Elastic License 2.0. The
default throughout the repository is a license under the Elastic License 2.0,
unless a file header or a license file in a subdirectory specifies another
license. [See the LICENSE](./LICENSE) for the full license text.
