use anyhow::Context;
use aws_sdk_sqs::types::QueueAttributeName;
use hiraeth_iam::AuthorizationMode;
use hiraeth_store::sqs::{SqsQueue, SqsStore};
use hiraeth_store_sqlx::SqlxStore;
use sqlx::SqlitePool;

mod common;

use common::{
    DEFAULT_REGION, queue_name, sdk_config_with_credentials, sqs_test_server,
    sqs_test_server_with_auth_mode, string_attribute,
};

use crate::common::batch_entry;

const TEST_ACCOUNT_ID: &str = "000000000000";
const LIMITED_ACCESS_KEY_ID: &str = "limited-test";
const LIMITED_SECRET_ACCESS_KEY: &str = "limited-test-secret";

#[tokio::test]
async fn create_queue_then_get_queue_url() -> anyhow::Result<()> {
    let server = sqs_test_server().await?;
    let queue_name = queue_name("create-get");

    let created = server
        .client
        .create_queue()
        .queue_name(&queue_name)
        .send()
        .await
        .context("create queue should succeed")?;
    let created_url = created
        .queue_url()
        .context("created queue should have url")?;

    let fetched = server
        .client
        .get_queue_url()
        .queue_name(&queue_name)
        .send()
        .await
        .context("get queue url should succeed")?;

    assert_eq!(fetched.queue_url(), Some(created_url));
    Ok(())
}

#[tokio::test]
async fn send_message_then_receive_and_delete_message() -> anyhow::Result<()> {
    let server = sqs_test_server().await?;
    let queue_url = server
        .client
        .create_queue()
        .queue_name(queue_name("send-receive"))
        .send()
        .await
        .context("create queue should succeed")?
        .queue_url()
        .context("created queue should have url")?
        .to_string();

    let sent = server
        .client
        .send_message()
        .queue_url(&queue_url)
        .message_body("hello from the aws rust sdk")
        .message_attributes("trace_id", string_attribute("abc123"))
        .send()
        .await
        .context("send message should succeed")?;
    assert!(sent.message_id().is_some());
    assert!(sent.md5_of_message_attributes().is_some());

    let received = server
        .client
        .receive_message()
        .queue_url(&queue_url)
        .message_attribute_names("All")
        .max_number_of_messages(1)
        .send()
        .await
        .context("receive message should succeed")?;
    let message = received
        .messages()
        .first()
        .context("one message should be returned")?;

    assert_eq!(message.body(), Some("hello from the aws rust sdk"));
    assert_eq!(
        message
            .message_attributes()
            .and_then(|attributes| attributes.get("trace_id"))
            .and_then(|attribute| attribute.string_value()),
        Some("abc123")
    );

    server
        .client
        .delete_message()
        .queue_url(&queue_url)
        .receipt_handle(
            message
                .receipt_handle()
                .context("received message should have receipt handle")?,
        )
        .send()
        .await
        .context("delete message should succeed")?;

    let after_delete = server
        .client
        .receive_message()
        .queue_url(&queue_url)
        .max_number_of_messages(1)
        .send()
        .await
        .context("receive after delete should succeed")?;
    assert!(after_delete.messages().is_empty());

    Ok(())
}

#[tokio::test]
async fn send_message_batch_returns_successful_entries() -> anyhow::Result<()> {
    let server = sqs_test_server().await?;
    let queue_url = server
        .client
        .create_queue()
        .queue_name(queue_name("batch"))
        .send()
        .await
        .context("create queue should succeed")?
        .queue_url()
        .context("created queue should have url")?
        .to_string();

    let response = server
        .client
        .send_message_batch()
        .queue_url(&queue_url)
        .entries(batch_entry("first", "one").build()?)
        .entries(batch_entry("second", "two").build()?)
        .send()
        .await
        .context("send message batch should succeed")?;

    assert_eq!(response.successful().len(), 2);
    assert!(response.failed().is_empty());
    assert_eq!(response.successful()[0].id(), "first");
    assert!(!response.successful()[0].message_id().is_empty());

    Ok(())
}

#[tokio::test]
async fn list_queues_respects_prefix() -> anyhow::Result<()> {
    let server = sqs_test_server().await?;
    let matching_one = queue_name("list-match");
    let matching_two = queue_name("list-match");
    let other = queue_name("list-other");

    for name in [&matching_one, &matching_two, &other] {
        server
            .client
            .create_queue()
            .queue_name(name)
            .send()
            .await
            .with_context(|| format!("create queue {name} should succeed"))?;
    }

    let response = server
        .client
        .list_queues()
        .queue_name_prefix("list-match")
        .send()
        .await
        .context("list queues should succeed")?;

    let urls = response.queue_urls();
    assert_eq!(urls.len(), 2);
    assert!(urls.iter().any(|url| url.ends_with(&matching_one)));
    assert!(urls.iter().any(|url| url.ends_with(&matching_two)));
    assert!(!urls.iter().any(|url| url.ends_with(&other)));

    Ok(())
}

#[tokio::test]
async fn set_and_get_queue_attributes_round_trip() -> anyhow::Result<()> {
    let server = sqs_test_server().await?;
    let queue_url = server
        .client
        .create_queue()
        .queue_name(queue_name("attributes"))
        .send()
        .await
        .context("create queue should succeed")?
        .queue_url()
        .context("created queue should have url")?
        .to_string();

    server
        .client
        .set_queue_attributes()
        .queue_url(&queue_url)
        .attributes(QueueAttributeName::VisibilityTimeout, "45")
        .attributes(QueueAttributeName::ReceiveMessageWaitTimeSeconds, "2")
        .send()
        .await
        .context("set queue attributes should succeed")?;

    let response = server
        .client
        .get_queue_attributes()
        .queue_url(&queue_url)
        .attribute_names(QueueAttributeName::VisibilityTimeout)
        .attribute_names(QueueAttributeName::ReceiveMessageWaitTimeSeconds)
        .send()
        .await
        .context("get queue attributes should succeed")?;

    let attributes = response
        .attributes()
        .context("response should include attributes")?;
    assert_eq!(
        attributes.get(&QueueAttributeName::VisibilityTimeout),
        Some(&"45".to_string())
    );
    assert_eq!(
        attributes.get(&QueueAttributeName::ReceiveMessageWaitTimeSeconds),
        Some(&"2".to_string())
    );

    Ok(())
}

#[tokio::test]
async fn queue_tags_round_trip() -> anyhow::Result<()> {
    let server = sqs_test_server().await?;
    let queue_url = server
        .client
        .create_queue()
        .queue_name(queue_name("tags"))
        .tags("environment", "test")
        .send()
        .await
        .context("create queue with tags should succeed")?
        .queue_url()
        .context("created queue should have url")?
        .to_string();

    server
        .client
        .tag_queue()
        .queue_url(&queue_url)
        .tags("owner", "hiraeth")
        .tags("environment", "integration")
        .send()
        .await
        .context("tag queue should succeed")?;

    let response = server
        .client
        .list_queue_tags()
        .queue_url(&queue_url)
        .send()
        .await
        .context("list queue tags should succeed")?;
    let tags = response.tags().context("response should include tags")?;
    assert_eq!(tags.get("environment"), Some(&"integration".to_string()));
    assert_eq!(tags.get("owner"), Some(&"hiraeth".to_string()));

    server
        .client
        .untag_queue()
        .queue_url(&queue_url)
        .tag_keys("owner")
        .send()
        .await
        .context("untag queue should succeed")?;

    let response = server
        .client
        .list_queue_tags()
        .queue_url(&queue_url)
        .send()
        .await
        .context("list queue tags after untag should succeed")?;
    let tags = response.tags().context("response should include tags")?;
    assert_eq!(tags.get("environment"), Some(&"integration".to_string()));
    assert!(!tags.contains_key("owner"));

    Ok(())
}

#[tokio::test]
async fn purge_queue_removes_messages() -> anyhow::Result<()> {
    let server = sqs_test_server().await?;
    let queue_url = server
        .client
        .create_queue()
        .queue_name(queue_name("purge"))
        .send()
        .await
        .context("create queue should succeed")?
        .queue_url()
        .context("created queue should have url")?
        .to_string();

    server
        .client
        .send_message()
        .queue_url(&queue_url)
        .message_body("to be purged")
        .send()
        .await
        .context("send message should succeed")?;

    server
        .client
        .purge_queue()
        .queue_url(&queue_url)
        .send()
        .await
        .context("purge queue should succeed")?;

    let response = server
        .client
        .receive_message()
        .queue_url(&queue_url)
        .max_number_of_messages(1)
        .send()
        .await
        .context("receive after purge should succeed")?;

    assert!(response.messages().is_empty());
    Ok(())
}

#[tokio::test]
async fn get_queue_url_missing_queue_maps_to_sdk_error() -> anyhow::Result<()> {
    let server = sqs_test_server().await?;
    let result = server
        .client
        .get_queue_url()
        .queue_name(queue_name("missing"))
        .send()
        .await;

    let error = result.expect_err("missing queue should return an SDK error");
    let service_error = error.into_service_error();
    assert!(
        service_error.is_queue_does_not_exist(),
        "expected QueueDoesNotExist, got {service_error:?}"
    );

    Ok(())
}

#[tokio::test]
async fn enforce_mode_denies_send_message_without_matching_queue_policy() -> anyhow::Result<()> {
    let server = sqs_test_server_with_auth_mode(AuthorizationMode::Enforce).await?;
    let queue_name = queue_name("authz-deny");
    seed_queue(server.server.db_url(), &queue_name, "{}").await?;
    seed_unprivileged_user(
        server.server.db_url(),
        "limited-test-user",
        LIMITED_ACCESS_KEY_ID,
        LIMITED_SECRET_ACCESS_KEY,
    )
    .await?;
    let queue_url = queue_url(server.server.endpoint_url(), &queue_name);
    let sdk_config = sdk_config_with_credentials(
        server.server.endpoint_url(),
        DEFAULT_REGION,
        LIMITED_ACCESS_KEY_ID,
        LIMITED_SECRET_ACCESS_KEY,
    )
    .await;
    let limited_client = aws_sdk_sqs::Client::new(&sdk_config);

    let result = limited_client
        .send_message()
        .queue_url(queue_url)
        .message_body("denied in enforce mode")
        .send()
        .await;

    let error = result.expect_err("send message should be denied without a matching policy");
    assert_eq!(
        error
            .raw_response()
            .map(|response| response.status().as_u16()),
        Some(403)
    );

    Ok(())
}

#[tokio::test]
async fn enforce_mode_allows_send_message_with_matching_queue_policy() -> anyhow::Result<()> {
    let server = sqs_test_server_with_auth_mode(AuthorizationMode::Enforce).await?;
    let queue_name = queue_name("authz-allow");
    let queue_arn = format!("arn:aws:sqs:{DEFAULT_REGION}:{TEST_ACCOUNT_ID}:{queue_name}");
    let policy = format!(
        r#"{{
            "Version": "2012-10-17",
            "Statement": [
                {{
                    "Effect": "Allow",
                    "Principal": {{
                        "AWS": "arn:aws:iam::{account_id}:user/test"
                    }},
                    "Action": "sqs:SendMessage",
                    "Resource": "{queue_arn}"
                }}
            ]
        }}"#,
        account_id = TEST_ACCOUNT_ID,
    );
    seed_queue(server.server.db_url(), &queue_name, &policy).await?;
    let queue_url = queue_url(server.server.endpoint_url(), &queue_name);

    let response = server
        .client
        .send_message()
        .queue_url(&queue_url)
        .message_body("allowed in enforce mode")
        .send()
        .await
        .context("send message should be allowed with a matching queue policy")?;

    assert!(response.message_id().is_some());

    let store = SqlxStore::new(server.server.db_url())
        .await
        .context("store should open for verification")?;
    let queue = store
        .sqs_store
        .get_queue(&queue_name, DEFAULT_REGION, TEST_ACCOUNT_ID)
        .await
        .context("queue lookup should succeed")?
        .context("seeded queue should exist")?;
    let message_count = store
        .sqs_store
        .get_message_count(queue.id)
        .await
        .context("message count lookup should succeed")?;

    assert_eq!(message_count, 1);

    Ok(())
}

async fn seed_queue(db_url: &str, queue_name: &str, policy: &str) -> anyhow::Result<()> {
    let store = SqlxStore::new(db_url)
        .await
        .context("store should open for queue seeding")?;
    let queue = SqsQueue {
        name: queue_name.to_string(),
        region: DEFAULT_REGION.to_string(),
        account_id: TEST_ACCOUNT_ID.to_string(),
        policy: policy.to_string(),
        ..SqsQueue::default()
    };

    store
        .sqs_store
        .create_queue(queue)
        .await
        .context("seed queue should be created")?;

    Ok(())
}

async fn seed_unprivileged_user(
    db_url: &str,
    user_name: &str,
    access_key_id: &str,
    secret_access_key: &str,
) -> anyhow::Result<()> {
    let pool = SqlitePool::connect(db_url)
        .await
        .context("sqlite pool should open for iam user seeding")?;

    let principal_id = sqlx::query!(
        "INSERT INTO iam_principals (account_id, kind, name) VALUES (?, ?, ?)",
        TEST_ACCOUNT_ID,
        "user",
        user_name
    )
    .execute(&pool)
    .await
    .context("principal should insert for authz deny test")?
    .last_insert_rowid();

    sqlx::query!(
        "INSERT INTO iam_access_keys (key_id, secret_key, principal_id) VALUES (?, ?, ?)",
        access_key_id,
        secret_access_key,
        principal_id
    )
    .execute(&pool)
    .await
    .context("access key should insert for authz deny test")?;

    Ok(())
}

fn queue_url(endpoint_url: &str, queue_name: &str) -> String {
    format!("{endpoint_url}/{TEST_ACCOUNT_ID}/{queue_name}")
}
