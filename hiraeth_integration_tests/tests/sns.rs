use anyhow::Context;
use aws_sdk_sqs::Client as SqsClient;

mod common;

use common::{DEFAULT_REGION, queue_name, sns_test_server, topic_name};

const TEST_ACCOUNT_ID: &str = "000000000000";

#[tokio::test]
#[ignore = "ListTopics action not yet implemented"]
async fn create_topic_then_list_topics() -> anyhow::Result<()> {
    let server = sns_test_server().await?;
    let topic_name = topic_name("create-list");

    let created = server
        .client
        .create_topic()
        .name(&topic_name)
        .send()
        .await
        .context("create topic should succeed")?;
    let arn = created.topic_arn().context("topic arn should be present")?;
    assert!(arn.contains(&topic_name));

    let listed = server
        .client
        .list_topics()
        .send()
        .await
        .context("list topics should succeed")?;
    let topics = listed.topics();
    assert!(topics.iter().any(|t| t.topic_arn() == Some(arn)));

    Ok(())
}

#[tokio::test]
#[ignore = "GetTopicAttributes action not yet implemented"]
async fn create_topic_then_get_topic_attributes() -> anyhow::Result<()> {
    let server = sns_test_server().await?;
    let topic_name = topic_name("create-attributes");

    let created = server
        .client
        .create_topic()
        .name(&topic_name)
        .send()
        .await
        .context("create topic should succeed")?;
    let arn = created.topic_arn().context("topic arn should be present")?;

    let response = server
        .client
        .get_topic_attributes()
        .topic_arn(arn)
        .send()
        .await
        .context("get topic attributes should succeed")?;
    let attributes = response
        .attributes()
        .context("response should include attributes")?;
    assert_eq!(attributes.get("TopicArn"), Some(&arn.to_string()));

    Ok(())
}

#[tokio::test]
#[ignore = "DeleteTopic and ListTopics actions not yet implemented"]
async fn delete_topic_removes_from_list() -> anyhow::Result<()> {
    let server = sns_test_server().await?;
    let topic_name = topic_name("delete-list");

    let created = server
        .client
        .create_topic()
        .name(&topic_name)
        .send()
        .await
        .context("create topic should succeed")?;
    let arn = created.topic_arn().context("topic arn should be present")?;

    server
        .client
        .delete_topic()
        .topic_arn(arn)
        .send()
        .await
        .context("delete topic should succeed")?;

    let listed = server
        .client
        .list_topics()
        .send()
        .await
        .context("list topics should succeed")?;
    let topics = listed.topics();
    assert!(!topics.iter().any(|t| t.topic_arn() == Some(arn)));

    Ok(())
}

#[tokio::test]
#[ignore = "ListSubscriptionsByTopic action not yet implemented"]
async fn subscribe_then_list_subscriptions() -> anyhow::Result<()> {
    let server = sns_test_server().await?;
    let topic_name = topic_name("subscribe-list");

    let created = server
        .client
        .create_topic()
        .name(&topic_name)
        .send()
        .await
        .context("create topic should succeed")?;
    let topic_arn = created.topic_arn().context("topic arn should be present")?;

    let queue_arn = format!("arn:aws:sqs:{DEFAULT_REGION}:{TEST_ACCOUNT_ID}:test-queue");
    let subscribed = server
        .client
        .subscribe()
        .topic_arn(topic_arn)
        .protocol("sqs")
        .endpoint(&queue_arn)
        .send()
        .await
        .context("subscribe should succeed")?;
    let subscription_arn = subscribed
        .subscription_arn()
        .context("subscription arn should be present")?;

    let listed = server
        .client
        .list_subscriptions_by_topic()
        .topic_arn(topic_arn)
        .send()
        .await
        .context("list subscriptions should succeed")?;
    let subscriptions = listed.subscriptions();
    assert!(
        subscriptions
            .iter()
            .any(|s| s.subscription_arn() == Some(subscription_arn))
    );

    Ok(())
}

#[tokio::test]
async fn publish_to_topic_with_subscriptions() -> anyhow::Result<()> {
    let server = sns_test_server().await?;
    let topic_name = topic_name("publish-with-sub");
    let q_name = queue_name("publish-with-sub");

    let created = server
        .client
        .create_topic()
        .name(&topic_name)
        .send()
        .await
        .context("create topic should succeed")?;
    let topic_arn = created.topic_arn().context("topic arn should be present")?;

    let sdk_config = server.server.sdk_config().await;
    let sqs_client = SqsClient::new(&sdk_config);

    let queue_url = sqs_client
        .create_queue()
        .queue_name(&q_name)
        .send()
        .await
        .context("create queue should succeed")?
        .queue_url()
        .context("queue url should be present")?
        .to_string();

    let queue_arn = format!("arn:aws:sqs:{DEFAULT_REGION}:{TEST_ACCOUNT_ID}:{q_name}");
    server
        .client
        .subscribe()
        .topic_arn(topic_arn)
        .protocol("sqs")
        .endpoint(&queue_arn)
        .send()
        .await
        .context("subscribe should succeed")?;

    let published = server
        .client
        .publish()
        .topic_arn(topic_arn)
        .message("hello from sns")
        .send()
        .await
        .context("publish should succeed")?;
    assert!(
        !published
            .message_id()
            .expect("message id should be present")
            .is_empty()
    );

    let received = sqs_client
        .receive_message()
        .queue_url(&queue_url)
        .max_number_of_messages(1)
        .send()
        .await
        .context("receive message should succeed")?;
    let message = received
        .messages()
        .first()
        .context("one message should be returned")?;
    assert!(message.body().unwrap_or("").contains("hello from sns"));

    Ok(())
}

#[tokio::test]
async fn publish_to_topic_without_subscriptions() -> anyhow::Result<()> {
    let server = sns_test_server().await?;
    let topic_name = topic_name("publish-no-sub");

    let created = server
        .client
        .create_topic()
        .name(&topic_name)
        .send()
        .await
        .context("create topic should succeed")?;
    let topic_arn = created.topic_arn().context("topic arn should be present")?;

    let published = server
        .client
        .publish()
        .topic_arn(topic_arn)
        .message("hello to nobody")
        .send()
        .await
        .context("publish should succeed")?;
    assert!(
        !published
            .message_id()
            .expect("message id should be present")
            .is_empty()
    );

    Ok(())
}
