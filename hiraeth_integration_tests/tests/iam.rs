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
async fn create_user_request_preserves_requested_path() -> anyhow::Result<()> {
    let server = iam_test_server().await?;
    let user_name = unique_name("integration-test-path-user");

    let response = server
        .client
        .create_user()
        .user_name(&user_name)
        .path("/engineering/local/")
        .send()
        .await?;

    let user = response
        .user()
        .context("create user response should include user")?;

    assert_eq!(user.user_name(), user_name.as_str());
    assert_eq!(user.path(), "/engineering/local/");
    assert_eq!(
        user.arn(),
        format!("arn:aws:iam::000000000000:user/engineering/local/{user_name}")
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

#[tokio::test]
async fn create_access_key_without_user_name_targets_signing_user() -> anyhow::Result<()> {
    let server = iam_test_server().await?;

    let response = server.client.create_access_key().send().await?;
    let access_key = response
        .access_key()
        .context("create access key response should include access key")?;

    assert_eq!(access_key.user_name(), "test");
    assert!(access_key.access_key_id().starts_with("AKIA"));
    assert_eq!(access_key.secret_access_key().len(), 40);

    Ok(())
}

#[tokio::test]
async fn get_user_request_returns_user_to_aws_sdk() -> anyhow::Result<()> {
    let server = iam_test_server().await?;
    let user_name = unique_name("integration-test-get-user");

    server
        .client
        .create_user()
        .user_name(&user_name)
        .send()
        .await?;

    let response = server
        .client
        .get_user()
        .user_name(&user_name)
        .send()
        .await?;
    let user = response
        .user()
        .context("get user response should include user")?;

    assert_eq!(user.user_name(), user_name.as_str());
    assert_eq!(user.path(), "/");
    assert_eq!(
        user.arn(),
        format!("arn:aws:iam::000000000000:user/{user_name}")
    );

    Ok(())
}

#[tokio::test]
async fn get_user_without_user_name_returns_signing_user() -> anyhow::Result<()> {
    let server = iam_test_server().await?;

    let response = server.client.get_user().send().await?;
    let user = response
        .user()
        .context("get user response should include user")?;

    assert_eq!(user.user_name(), "test");
    assert_eq!(user.path(), "/");
    assert_eq!(user.arn(), "arn:aws:iam::000000000000:user/test");

    Ok(())
}

#[tokio::test]
async fn get_user_for_missing_user_returns_no_such_entity() -> anyhow::Result<()> {
    let server = iam_test_server().await?;
    let user_name = unique_name("integration-test-missing-user");

    let error = server
        .client
        .get_user()
        .user_name(&user_name)
        .send()
        .await
        .expect_err("missing user should return an SDK error");

    let service_error = error
        .as_service_error()
        .context("missing user should return a modeled service error")?;
    assert_eq!(service_error.meta().code(), Some("NoSuchEntity"));

    Ok(())
}

#[tokio::test]
async fn delete_user_removes_user_from_iam() -> anyhow::Result<()> {
    let server = iam_test_server().await?;
    let user_name = unique_name("integration-test");

    server
        .client
        .create_user()
        .user_name(&user_name)
        .send()
        .await?;

    server
        .client
        .delete_user()
        .user_name(&user_name)
        .send()
        .await?;

    let error = server
        .client
        .get_user()
        .user_name(&user_name)
        .send()
        .await
        .expect_err("deleted user should no longer be returned");

    let service_error = error
        .as_service_error()
        .context("deleted user lookup should return a modeled service error")?;
    assert_eq!(service_error.meta().code(), Some("NoSuchEntity"));

    Ok(())
}

#[tokio::test]
async fn delete_user_for_missing_user_returns_no_such_entity() -> anyhow::Result<()> {
    let server = iam_test_server().await?;
    let user_name = unique_name("integration-test-delete-missing-user");

    let error = server
        .client
        .delete_user()
        .user_name(&user_name)
        .send()
        .await
        .expect_err("deleting a missing user should return an SDK error");

    let service_error = error
        .as_service_error()
        .context("missing user delete should return a modeled service error")?;
    assert_eq!(service_error.meta().code(), Some("NoSuchEntity"));

    Ok(())
}
