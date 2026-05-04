use async_trait::async_trait;
use hiraeth_store::StoreError;
use hiraeth_store::sns::{SnsStore, SnsSubscription, SnsTopic};

#[derive(Clone)]
pub struct SqliteSnsStore {
    pool: sqlx::SqlitePool,
}

impl SqliteSnsStore {
    pub fn new(pool: &sqlx::SqlitePool) -> Self {
        Self { pool: pool.clone() }
    }
}

fn map_sqlx_error(err: sqlx::Error) -> StoreError {
    if let sqlx::Error::Database(database_error) = &err
        && database_error.is_unique_violation()
    {
        return StoreError::Conflict(database_error.message().to_string());
    }

    StoreError::StorageFailure(err.to_string())
}

#[async_trait]
impl SnsStore for SqliteSnsStore {
    async fn create_topic(&self, topic: SnsTopic) -> Result<(), StoreError> {
        sqlx::query!(
            "INSERT INTO sns_topics (
                name, region, account_id, display_name, policy,
                delivery_policy, fifo_topic, signature_version, tracing_config,
                kms_master_key_id, data_protection_policy, created_at
             ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
            topic.name,
            topic.region,
            topic.account_id,
            topic.display_name,
            topic.policy,
            topic.delivery_policy,
            topic.fifo_topic,
            topic.signature_version,
            topic.tracing_config,
            topic.kms_master_key_id,
            topic.data_protection_policy,
            topic.created_at
        )
        .execute(&self.pool)
        .await
        .map_err(map_sqlx_error)?;
        Ok(())
    }

    async fn get_topic(&self, topic_arn: &str) -> Result<Option<SnsTopic>, StoreError> {
        let parts: Vec<&str> = topic_arn.split(':').collect();
        if parts.len() != 6 {
            return Ok(None);
        }
        let region = parts[3];
        let account_id = parts[4];
        let name = parts[5];

        let topic = sqlx::query_as!(
            SnsTopic,
            "SELECT id as \"id!: i64\", name, region, account_id, display_name, policy,
                    delivery_policy, fifo_topic, signature_version, tracing_config,
                    kms_master_key_id, data_protection_policy, created_at
             FROM sns_topics
             WHERE name = ? AND region = ? AND account_id = ?",
            name,
            region,
            account_id
        )
        .fetch_optional(&self.pool)
        .await
        .map_err(map_sqlx_error)?;

        Ok(topic)
    }

    async fn get_topic_by_id(&self, id: i64) -> Result<Option<SnsTopic>, StoreError> {
        let topic = sqlx::query_as!(
            SnsTopic,
            "SELECT id as \"id!: i64\", name, region, account_id, display_name, policy,
                    delivery_policy, fifo_topic, signature_version, tracing_config,
                    kms_master_key_id, data_protection_policy, created_at
             FROM sns_topics
             WHERE id = ?",
            id
        )
        .fetch_optional(&self.pool)
        .await
        .map_err(map_sqlx_error)?;

        Ok(topic)
    }

    async fn list_topics(
        &self,
        region: &str,
        account_id: &str,
        prefix: Option<&str>,
        limit: Option<i64>,
    ) -> Result<Vec<SnsTopic>, StoreError> {
        let limit = limit.unwrap_or(100);
        let topics = if let Some(prefix) = prefix {
            sqlx::query_as!(
                SnsTopic,
                "SELECT id as \"id!: i64\", name, region, account_id, display_name, policy,
                        delivery_policy, fifo_topic, signature_version, tracing_config,
                        kms_master_key_id, data_protection_policy, created_at
                 FROM sns_topics
                 WHERE region = ? AND account_id = ? AND name LIKE ? || '%'
                 ORDER BY name
                 LIMIT ?",
                region,
                account_id,
                prefix,
                limit
            )
            .fetch_all(&self.pool)
            .await
        } else {
            sqlx::query_as!(
                SnsTopic,
                "SELECT id as \"id!: i64\", name, region, account_id, display_name, policy,
                        delivery_policy, fifo_topic, signature_version, tracing_config,
                        kms_master_key_id, data_protection_policy, created_at
                 FROM sns_topics
                 WHERE region = ? AND account_id = ?
                 ORDER BY name
                 LIMIT ?",
                region,
                account_id,
                limit
            )
            .fetch_all(&self.pool)
            .await
        }
        .map_err(map_sqlx_error)?;

        Ok(topics)
    }

    async fn delete_topic(&self, topic_arn: &str) -> Result<(), StoreError> {
        let parts: Vec<&str> = topic_arn.split(':').collect();
        if parts.len() != 6 {
            return Err(StoreError::NotFound("topic not found".to_string()));
        }
        let region = parts[3];
        let account_id = parts[4];
        let name = parts[5];

        let result = sqlx::query!(
            "DELETE FROM sns_topics WHERE name = ? AND region = ? AND account_id = ?",
            name,
            region,
            account_id
        )
        .execute(&self.pool)
        .await
        .map_err(map_sqlx_error)?;

        if result.rows_affected() == 0 {
            return Err(StoreError::NotFound("topic not found".to_string()));
        }

        sqlx::query!(
            "DELETE FROM sns_subscriptions WHERE topic_arn = ?",
            topic_arn
        )
        .execute(&self.pool)
        .await
        .map_err(map_sqlx_error)?;

        Ok(())
    }

    async fn create_subscription(&self, subscription: SnsSubscription) -> Result<(), StoreError> {
        sqlx::query!(
            "INSERT INTO sns_subscriptions (
                topic_arn, protocol, endpoint, owner_account_id, subscription_arn,
                delivery_policy, filter_policy, filter_policy_scope, raw_message_delivery,
                redrive_policy, subscription_role_arn, replay_policy, created_at
             ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
            subscription.topic_arn,
            subscription.protocol,
            subscription.endpoint,
            subscription.owner_account_id,
            subscription.subscription_arn,
            subscription.delivery_policy,
            subscription.filter_policy,
            subscription.filter_policy_scope,
            subscription.raw_message_delivery,
            subscription.redrive_policy,
            subscription.subscription_role_arn,
            subscription.replay_policy,
            subscription.created_at
        )
        .execute(&self.pool)
        .await
        .map_err(map_sqlx_error)?;
        Ok(())
    }

    async fn get_subscription(
        &self,
        subscription_arn: &str,
    ) -> Result<Option<SnsSubscription>, StoreError> {
        let sub = sqlx::query_as!(
            SnsSubscription,
            "SELECT id as \"id!: i64\", topic_arn, protocol, endpoint, owner_account_id, subscription_arn,
                    delivery_policy, filter_policy, filter_policy_scope, raw_message_delivery,
                    redrive_policy, subscription_role_arn, replay_policy, created_at
             FROM sns_subscriptions
             WHERE subscription_arn = ?",
            subscription_arn
        )
        .fetch_optional(&self.pool)
        .await
        .map_err(map_sqlx_error)?;

        Ok(sub)
    }

    async fn get_subscription_by_id(&self, id: i64) -> Result<Option<SnsSubscription>, StoreError> {
        let sub = sqlx::query_as!(
            SnsSubscription,
            "SELECT id as \"id!: i64\", topic_arn, protocol, endpoint, owner_account_id, subscription_arn,
                    delivery_policy, filter_policy, filter_policy_scope, raw_message_delivery,
                    redrive_policy, subscription_role_arn, replay_policy, created_at
             FROM sns_subscriptions
             WHERE id = ?",
            id
        )
        .fetch_optional(&self.pool)
        .await
        .map_err(map_sqlx_error)?;

        Ok(sub)
    }

    async fn list_subscriptions_by_topic(
        &self,
        topic_arn: &str,
    ) -> Result<Vec<SnsSubscription>, StoreError> {
        let subs = sqlx::query_as!(
            SnsSubscription,
            "SELECT id as \"id!: i64\", topic_arn, protocol, endpoint, owner_account_id, subscription_arn,
                    delivery_policy, filter_policy, filter_policy_scope, raw_message_delivery,
                    redrive_policy, subscription_role_arn, replay_policy, created_at
             FROM sns_subscriptions
             WHERE topic_arn = ?",
            topic_arn
        )
        .fetch_all(&self.pool)
        .await
        .map_err(map_sqlx_error)?;

        Ok(subs)
    }

    async fn delete_subscription(&self, subscription_arn: &str) -> Result<(), StoreError> {
        let result = sqlx::query!(
            "DELETE FROM sns_subscriptions WHERE subscription_arn = ?",
            subscription_arn
        )
        .execute(&self.pool)
        .await
        .map_err(map_sqlx_error)?;

        if result.rows_affected() == 0 {
            return Err(StoreError::NotFound("subscription not found".to_string()));
        }

        Ok(())
    }

    async fn delete_subscription_by_id(&self, id: i64) -> Result<(), StoreError> {
        let result = sqlx::query!("DELETE FROM sns_subscriptions WHERE id = ?", id)
            .execute(&self.pool)
            .await
            .map_err(map_sqlx_error)?;

        if result.rows_affected() == 0 {
            return Err(StoreError::NotFound("subscription not found".to_string()));
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use chrono::Utc;
    use tempfile::TempDir;

    use super::SqliteSnsStore;
    use crate::{get_store_pool, run_migrations};
    use hiraeth_store::{
        StoreError,
        sns::{SnsStore, SnsSubscription, SnsTopic},
    };

    async fn test_store() -> (TempDir, SqliteSnsStore) {
        let temp_dir = TempDir::new().expect("temp dir should be created");
        let db_url = format!(
            "sqlite://{}",
            temp_dir.path().join("store-test.sqlite").display()
        );
        let pool = get_store_pool(&db_url)
            .await
            .expect("pool should connect to temp sqlite db");
        run_migrations(&pool)
            .await
            .expect("migrations should run for temp sqlite db");
        (temp_dir, SqliteSnsStore::new(&pool))
    }

    fn topic(name: &str, region: &str, account_id: &str) -> SnsTopic {
        let now = Utc::now().naive_utc();
        SnsTopic {
            id: 0,
            name: name.to_string(),
            region: region.to_string(),
            account_id: account_id.to_string(),
            display_name: None,
            policy: "{}".to_string(),
            delivery_policy: None,
            fifo_topic: None,
            signature_version: None,
            tracing_config: None,
            kms_master_key_id: None,
            data_protection_policy: None,
            created_at: now,
        }
    }

    fn subscription(
        topic_arn: &str,
        protocol: &str,
        endpoint: &str,
        owner_account_id: &str,
        subscription_arn: &str,
    ) -> SnsSubscription {
        let now = Utc::now().naive_utc();
        SnsSubscription {
            id: 0,
            topic_arn: topic_arn.to_string(),
            protocol: protocol.to_string(),
            endpoint: endpoint.to_string(),
            owner_account_id: owner_account_id.to_string(),
            subscription_arn: subscription_arn.to_string(),
            delivery_policy: None,
            filter_policy: None,
            filter_policy_scope: None,
            raw_message_delivery: None,
            redrive_policy: None,
            subscription_role_arn: None,
            replay_policy: None,
            created_at: now,
        }
    }

    fn topic_arn(name: &str, region: &str, account_id: &str) -> String {
        format!("arn:aws:sns:{region}:{account_id}:{name}")
    }

    #[tokio::test]
    async fn run_migrations_creates_sns_topics_table() {
        let (_temp_dir, store) = test_store().await;

        let t = topic("my-topic", "us-east-1", "123456789012");
        store
            .create_topic(t)
            .await
            .expect("topic should insert after migrations");

        let found = store
            .get_topic(&topic_arn("my-topic", "us-east-1", "123456789012"))
            .await
            .expect("get topic should succeed")
            .expect("topic should exist");

        assert_eq!(found.name, "my-topic");
    }

    #[tokio::test]
    async fn run_migrations_creates_sns_subscriptions_table() {
        let (_temp_dir, store) = test_store().await;

        let t = topic("my-topic", "us-east-1", "123456789012");
        store.create_topic(t).await.expect("topic should insert");

        let arn = topic_arn("my-topic", "us-east-1", "123456789012");
        let sub = subscription(
            &arn,
            "sqs",
            "arn:aws:sqs:us-east-1:123456789012:queue",
            "123456789012",
            "arn:aws:sns:us-east-1:123456789012:my-topic:sub-1",
        );
        store
            .create_subscription(sub)
            .await
            .expect("subscription should insert after migrations");

        let found = store
            .get_subscription("arn:aws:sns:us-east-1:123456789012:my-topic:sub-1")
            .await
            .expect("get subscription should succeed")
            .expect("subscription should exist");

        assert_eq!(found.topic_arn, arn);
    }

    #[tokio::test]
    async fn create_topic_and_get_topic_round_trip() {
        let (_temp_dir, store) = test_store().await;
        let expected = SnsTopic {
            display_name: Some("My Display Name".to_string()),
            policy: r#"{"Statement":[]}"#.to_string(),
            delivery_policy: Some(r#"{"healthyRetryPolicy":{"numRetries":3}}"#.to_string()),
            fifo_topic: Some("true".to_string()),
            signature_version: Some("2".to_string()),
            tracing_config: Some("Active".to_string()),
            kms_master_key_id: Some("alias/test".to_string()),
            data_protection_policy: Some(r#"{"Name":"policy"}"#.to_string()),
            ..topic("round-trip-topic", "us-east-1", "123456789012")
        };

        store
            .create_topic(expected.clone())
            .await
            .expect("topic should insert");

        let found = store
            .get_topic(&topic_arn("round-trip-topic", "us-east-1", "123456789012"))
            .await
            .expect("get topic should succeed")
            .expect("topic should exist");

        assert!(found.id > 0);
        assert_eq!(found.name, expected.name);
        assert_eq!(found.region, expected.region);
        assert_eq!(found.account_id, expected.account_id);
        assert_eq!(found.display_name, expected.display_name);
        assert_eq!(found.policy, expected.policy);
        assert_eq!(found.delivery_policy, expected.delivery_policy);
        assert_eq!(found.fifo_topic, expected.fifo_topic);
        assert_eq!(found.signature_version, expected.signature_version);
        assert_eq!(found.tracing_config, expected.tracing_config);
        assert_eq!(found.kms_master_key_id, expected.kms_master_key_id);
        assert_eq!(
            found.data_protection_policy,
            expected.data_protection_policy
        );
        assert_eq!(found.created_at, expected.created_at);
    }

    #[tokio::test]
    async fn get_topic_by_id_returns_correct_topic() {
        let (_temp_dir, store) = test_store().await;

        store
            .create_topic(topic("my-topic", "us-east-1", "123456789012"))
            .await
            .expect("topic should insert");

        let by_arn = store
            .get_topic(&topic_arn("my-topic", "us-east-1", "123456789012"))
            .await
            .expect("get topic should succeed")
            .expect("topic should exist");

        let by_id = store
            .get_topic_by_id(by_arn.id)
            .await
            .expect("get topic by id should succeed")
            .expect("topic should exist");

        assert_eq!(by_id, by_arn);
    }

    #[tokio::test]
    async fn list_topics_filters_by_region_and_account() {
        let (_temp_dir, store) = test_store().await;

        for t in [
            topic("topic-a", "us-east-1", "123456789012"),
            topic("topic-b", "us-east-1", "123456789012"),
            topic("topic-c", "us-west-2", "123456789012"),
            topic("topic-d", "us-east-1", "999999999999"),
        ] {
            store.create_topic(t).await.expect("topic should insert");
        }

        let topics = store
            .list_topics("us-east-1", "123456789012", None, None)
            .await
            .expect("list topics should succeed");

        let names: Vec<String> = topics.into_iter().map(|t| t.name).collect();
        assert_eq!(names, vec!["topic-a", "topic-b"]);
    }

    #[tokio::test]
    async fn list_topics_filters_by_prefix() {
        let (_temp_dir, store) = test_store().await;

        for t in [
            topic("orders-dev", "us-east-1", "123456789012"),
            topic("orders-prod", "us-east-1", "123456789012"),
            topic("billing-prod", "us-east-1", "123456789012"),
        ] {
            store.create_topic(t).await.expect("topic should insert");
        }

        let topics = store
            .list_topics("us-east-1", "123456789012", Some("orders-"), None)
            .await
            .expect("list topics should succeed");

        let names: Vec<String> = topics.into_iter().map(|t| t.name).collect();
        assert_eq!(names, vec!["orders-dev", "orders-prod"]);
    }

    #[tokio::test]
    async fn list_topics_respects_limit() {
        let (_temp_dir, store) = test_store().await;

        for t in [
            topic("alpha", "us-east-1", "123456789012"),
            topic("beta", "us-east-1", "123456789012"),
            topic("gamma", "us-east-1", "123456789012"),
        ] {
            store.create_topic(t).await.expect("topic should insert");
        }

        let topics = store
            .list_topics("us-east-1", "123456789012", None, Some(2))
            .await
            .expect("list topics should succeed");

        assert_eq!(topics.len(), 2);
    }

    #[tokio::test]
    async fn create_topic_rejects_duplicate() {
        let (_temp_dir, store) = test_store().await;

        let t = topic("my-topic", "us-east-1", "123456789012");
        store
            .create_topic(t.clone())
            .await
            .expect("first insert should succeed");

        let result = store.create_topic(t).await;
        assert!(matches!(result, Err(StoreError::Conflict(_))));
    }

    #[tokio::test]
    async fn delete_topic_removes_topic_and_subscriptions() {
        let (_temp_dir, store) = test_store().await;

        let t = topic("my-topic", "us-east-1", "123456789012");
        store.create_topic(t).await.expect("topic should insert");

        let arn = topic_arn("my-topic", "us-east-1", "123456789012");
        let sub = subscription(
            &arn,
            "sqs",
            "arn:aws:sqs:us-east-1:123456789012:queue",
            "123456789012",
            "arn:aws:sns:us-east-1:123456789012:my-topic:sub-1",
        );
        store
            .create_subscription(sub)
            .await
            .expect("subscription should insert");

        store
            .delete_topic(&arn)
            .await
            .expect("delete topic should succeed");

        let topic = store
            .get_topic(&arn)
            .await
            .expect("get topic should succeed");
        assert!(topic.is_none());

        let sub = store
            .get_subscription("arn:aws:sns:us-east-1:123456789012:my-topic:sub-1")
            .await
            .expect("get subscription should succeed");
        assert!(sub.is_none());
    }

    #[tokio::test]
    async fn create_subscription_and_get_subscription_round_trip() {
        let (_temp_dir, store) = test_store().await;

        let t = topic("my-topic", "us-east-1", "123456789012");
        store.create_topic(t).await.expect("topic should insert");

        let arn = topic_arn("my-topic", "us-east-1", "123456789012");
        let expected = SnsSubscription {
            delivery_policy: Some(r#"{"numRetries":3}"#.to_string()),
            filter_policy: Some(r#"{"sport":["rugby"]}"#.to_string()),
            filter_policy_scope: Some("MessageAttributes".to_string()),
            raw_message_delivery: Some("true".to_string()),
            redrive_policy: Some(
                r#"{"deadLetterTargetArn":"arn:aws:sqs:us-east-1:123456789012:dlq"}"#.to_string(),
            ),
            subscription_role_arn: Some("arn:aws:iam::123456789012:role/SNSRole".to_string()),
            replay_policy: Some(r#"{"numRetries":5}"#.to_string()),
            ..subscription(
                &arn,
                "sqs",
                "arn:aws:sqs:us-east-1:123456789012:queue",
                "123456789012",
                "arn:aws:sns:us-east-1:123456789012:my-topic:sub-1",
            )
        };

        store
            .create_subscription(expected.clone())
            .await
            .expect("subscription should insert");

        let found = store
            .get_subscription("arn:aws:sns:us-east-1:123456789012:my-topic:sub-1")
            .await
            .expect("get subscription should succeed")
            .expect("subscription should exist");

        assert!(found.id > 0);
        assert_eq!(found.topic_arn, expected.topic_arn);
        assert_eq!(found.protocol, expected.protocol);
        assert_eq!(found.endpoint, expected.endpoint);
        assert_eq!(found.owner_account_id, expected.owner_account_id);
        assert_eq!(found.subscription_arn, expected.subscription_arn);
        assert_eq!(found.delivery_policy, expected.delivery_policy);
        assert_eq!(found.filter_policy, expected.filter_policy);
        assert_eq!(found.filter_policy_scope, expected.filter_policy_scope);
        assert_eq!(found.raw_message_delivery, expected.raw_message_delivery);
        assert_eq!(found.redrive_policy, expected.redrive_policy);
        assert_eq!(found.subscription_role_arn, expected.subscription_role_arn);
        assert_eq!(found.replay_policy, expected.replay_policy);
        assert_eq!(found.created_at, expected.created_at);
    }

    #[tokio::test]
    async fn get_subscription_by_id_returns_correct_subscription() {
        let (_temp_dir, store) = test_store().await;

        let t = topic("my-topic", "us-east-1", "123456789012");
        store.create_topic(t).await.expect("topic should insert");

        let arn = topic_arn("my-topic", "us-east-1", "123456789012");
        let sub = subscription(
            &arn,
            "sqs",
            "arn:aws:sqs:us-east-1:123456789012:queue",
            "123456789012",
            "arn:aws:sns:us-east-1:123456789012:my-topic:sub-1",
        );
        store
            .create_subscription(sub)
            .await
            .expect("subscription should insert");

        let by_arn = store
            .get_subscription("arn:aws:sns:us-east-1:123456789012:my-topic:sub-1")
            .await
            .expect("get subscription should succeed")
            .expect("subscription should exist");

        let by_id = store
            .get_subscription_by_id(by_arn.id)
            .await
            .expect("get subscription by id should succeed")
            .expect("subscription should exist");

        assert_eq!(by_id, by_arn);
    }

    #[tokio::test]
    async fn list_subscriptions_by_topic_filters_correctly() {
        let (_temp_dir, store) = test_store().await;

        let t1 = topic("topic-1", "us-east-1", "123456789012");
        let t2 = topic("topic-2", "us-east-1", "123456789012");
        store.create_topic(t1).await.expect("topic 1 should insert");
        store.create_topic(t2).await.expect("topic 2 should insert");

        let arn1 = topic_arn("topic-1", "us-east-1", "123456789012");
        let arn2 = topic_arn("topic-2", "us-east-1", "123456789012");

        let sub1 = subscription(
            &arn1,
            "sqs",
            "arn:aws:sqs:us-east-1:123456789012:queue1",
            "123456789012",
            "arn:aws:sns:us-east-1:123456789012:topic-1:sub-1",
        );
        let sub2 = subscription(
            &arn1,
            "sqs",
            "arn:aws:sqs:us-east-1:123456789012:queue2",
            "123456789012",
            "arn:aws:sns:us-east-1:123456789012:topic-1:sub-2",
        );
        let sub3 = subscription(
            &arn2,
            "sqs",
            "arn:aws:sqs:us-east-1:123456789012:queue3",
            "123456789012",
            "arn:aws:sns:us-east-1:123456789012:topic-2:sub-1",
        );

        store
            .create_subscription(sub1)
            .await
            .expect("sub 1 should insert");
        store
            .create_subscription(sub2)
            .await
            .expect("sub 2 should insert");
        store
            .create_subscription(sub3)
            .await
            .expect("sub 3 should insert");

        let subs = store
            .list_subscriptions_by_topic(&arn1)
            .await
            .expect("list subscriptions should succeed");

        let arns: Vec<String> = subs.into_iter().map(|s| s.subscription_arn).collect();
        assert_eq!(arns.len(), 2);
        assert!(arns.contains(&"arn:aws:sns:us-east-1:123456789012:topic-1:sub-1".to_string()));
        assert!(arns.contains(&"arn:aws:sns:us-east-1:123456789012:topic-1:sub-2".to_string()));
    }

    #[tokio::test]
    async fn delete_subscription_removes_subscription() {
        let (_temp_dir, store) = test_store().await;

        let t = topic("my-topic", "us-east-1", "123456789012");
        store.create_topic(t).await.expect("topic should insert");

        let arn = topic_arn("my-topic", "us-east-1", "123456789012");
        let sub = subscription(
            &arn,
            "sqs",
            "arn:aws:sqs:us-east-1:123456789012:queue",
            "123456789012",
            "arn:aws:sns:us-east-1:123456789012:my-topic:sub-1",
        );
        store
            .create_subscription(sub)
            .await
            .expect("subscription should insert");

        store
            .delete_subscription("arn:aws:sns:us-east-1:123456789012:my-topic:sub-1")
            .await
            .expect("delete subscription should succeed");

        let found = store
            .get_subscription("arn:aws:sns:us-east-1:123456789012:my-topic:sub-1")
            .await
            .expect("get subscription should succeed");

        assert!(found.is_none());
    }

    #[tokio::test]
    async fn delete_subscription_by_id_removes_subscription() {
        let (_temp_dir, store) = test_store().await;

        let t = topic("my-topic", "us-east-1", "123456789012");
        store.create_topic(t).await.expect("topic should insert");

        let arn = topic_arn("my-topic", "us-east-1", "123456789012");
        let sub = subscription(
            &arn,
            "sqs",
            "arn:aws:sqs:us-east-1:123456789012:queue",
            "123456789012",
            "arn:aws:sns:us-east-1:123456789012:my-topic:sub-1",
        );
        store
            .create_subscription(sub)
            .await
            .expect("subscription should insert");

        let created = store
            .get_subscription("arn:aws:sns:us-east-1:123456789012:my-topic:sub-1")
            .await
            .expect("get subscription should succeed")
            .expect("subscription should exist");

        store
            .delete_subscription_by_id(created.id)
            .await
            .expect("delete subscription by id should succeed");

        let found = store
            .get_subscription_by_id(created.id)
            .await
            .expect("get subscription by id should succeed");

        assert!(found.is_none());
    }
}
