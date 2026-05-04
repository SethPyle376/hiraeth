#![allow(dead_code)]
use aws_sdk_sns::Client;
use hiraeth_iam::AuthorizationMode;

use crate::common::{TestServer, start_test_server, start_test_server_with_auth_mode, unique_name};

pub struct SnsTestServer {
    pub server: TestServer,
    pub client: Client,
}

pub async fn sns_test_server() -> anyhow::Result<SnsTestServer> {
    let server = start_test_server().await?;
    let sdk_config = server.sdk_config().await;

    Ok(SnsTestServer {
        client: Client::new(&sdk_config),
        server,
    })
}

pub async fn sns_test_server_with_auth_mode(
    auth_mode: AuthorizationMode,
) -> anyhow::Result<SnsTestServer> {
    let server = start_test_server_with_auth_mode(auth_mode).await?;
    let sdk_config = server.sdk_config().await;

    Ok(SnsTestServer {
        client: Client::new(&sdk_config),
        server,
    })
}

pub fn topic_name(prefix: &str) -> String {
    unique_name(prefix)
}
