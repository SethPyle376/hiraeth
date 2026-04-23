use anyhow::Context;

mod common;

use common::iam_test_server;

#[tokio::test]
async fn create_user_request_authenticates_and_routes_to_iam_service() -> anyhow::Result<()> {
    let server = iam_test_server().await?;

    let error = server
        .client
        .create_user()
        .user_name("integration-test-user")
        .send()
        .await
        .expect_err("create user should reach placeholder iam handler");

    let status = error
        .raw_response()
        .context("service error should expose raw response")?
        .status()
        .as_u16();

    assert_eq!(status, 501);

    Ok(())
}
