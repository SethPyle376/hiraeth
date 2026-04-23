use anyhow::Context;

mod common;

use common::{iam_test_server, unique_name};

#[tokio::test]
async fn create_user_request_returns_created_user() -> anyhow::Result<()> {
    let server = iam_test_server().await?;
    let user_name = unique_name("integration-test-user");

    let response = server
        .client
        .create_user()
        .user_name(&user_name)
        .send()
        .await?;

    let user = response
        .user()
        .context("create user response should include user")?;

    assert_eq!(user.user_name(), user_name.as_str());
    assert_eq!(user.path(), "/");
    assert!(user.user_id().starts_with("AIDA"));
    assert_eq!(
        user.arn(),
        format!("arn:aws:iam::000000000000:user/{user_name}")
    );

    Ok(())
}

#[tokio::test]
async fn create_access_key_request_returns_created_key_for_requested_user() -> anyhow::Result<()> {
    let server = iam_test_server().await?;
    let user_name = unique_name("integration-test-key-user");

    server
        .client
        .create_user()
        .user_name(&user_name)
        .send()
        .await?;

    let response = server
        .client
        .create_access_key()
        .user_name(&user_name)
        .send()
        .await?;

    let access_key = response
        .access_key()
        .context("create access key response should include access key")?;

    assert_eq!(access_key.user_name(), user_name.as_str());
    assert!(access_key.access_key_id().starts_with("AKIA"));
    assert_eq!(access_key.secret_access_key().len(), 40);

    Ok(())
}
