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
            "INSERT INTO sns_topics (name, region, account_id, display_name, policy, created_at)
             VALUES (?, ?, ?, ?, ?, ?)",
            topic.name,
            topic.region,
            topic.account_id,
            topic.display_name,
            topic.policy,
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
            "SELECT id as \"id!: i64\", name, region, account_id, display_name, policy, created_at
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
            "SELECT id as \"id!: i64\", name, region, account_id, display_name, policy, created_at
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
                "SELECT id as \"id!: i64\", name, region, account_id, display_name, policy, created_at
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
                "SELECT id as \"id!: i64\", name, region, account_id, display_name, policy, created_at
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
            "INSERT INTO sns_subscriptions (topic_arn, protocol, endpoint, owner_account_id, subscription_arn, created_at)
             VALUES (?, ?, ?, ?, ?, ?)",
            subscription.topic_arn,
            subscription.protocol,
            subscription.endpoint,
            subscription.owner_account_id,
            subscription.subscription_arn,
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
            "SELECT id as \"id!: i64\", topic_arn, protocol, endpoint, owner_account_id, subscription_arn, created_at
             FROM sns_subscriptions
             WHERE subscription_arn = ?",
            subscription_arn
        )
        .fetch_optional(&self.pool)
        .await
        .map_err(map_sqlx_error)?;

        Ok(sub)
    }

    async fn get_subscription_by_id(
        &self,
        id: i64,
    ) -> Result<Option<SnsSubscription>, StoreError> {
        let sub = sqlx::query_as!(
            SnsSubscription,
            "SELECT id as \"id!: i64\", topic_arn, protocol, endpoint, owner_account_id, subscription_arn, created_at
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
            "SELECT id as \"id!: i64\", topic_arn, protocol, endpoint, owner_account_id, subscription_arn, created_at
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
        let result = sqlx::query!(
            "DELETE FROM sns_subscriptions WHERE id = ?",
            id
        )
        .execute(&self.pool)
        .await
        .map_err(map_sqlx_error)?;

        if result.rows_affected() == 0 {
            return Err(StoreError::NotFound("subscription not found".to_string()));
        }

        Ok(())
    }
}
