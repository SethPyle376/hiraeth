#![allow(dead_code)]
use aws_sdk_iam::Client;
use hiraeth_iam::AuthorizationMode;

use crate::common::{TestServer, start_test_server, start_test_server_with_auth_mode};

pub struct IamTestServer {
    pub server: TestServer,
    pub client: Client,
}

pub async fn iam_test_server() -> anyhow::Result<IamTestServer> {
    let server = start_test_server().await?;
    let sdk_config = server.sdk_config().await;

    Ok(IamTestServer {
        client: Client::new(&sdk_config),
        server,
    })
}

pub async fn iam_test_server_with_auth_mode(
    auth_mode: AuthorizationMode,
) -> anyhow::Result<IamTestServer> {
    let server = start_test_server_with_auth_mode(auth_mode).await?;
    let sdk_config = server.sdk_config().await;

    Ok(IamTestServer {
        client: Client::new(&sdk_config),
        server,
    })
}
