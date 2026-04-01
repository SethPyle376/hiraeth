use async_trait::async_trait;
use hiraeth_store::sqs::{SqsQueue, SqsStore, SqsStoreError};

#[derive(Clone)]
pub struct SqliteSqsStore {
    pool: sqlx::SqlitePool,
}

impl SqliteSqsStore {
    pub fn new(pool: &sqlx::SqlitePool) -> Self {
        Self { pool: pool.clone() }
    }
}

#[async_trait]
impl SqsStore for SqliteSqsStore {
    async fn create_queue(
        &self,
        queue: hiraeth_store::sqs::SqsQueue,
    ) -> Result<(), hiraeth_store::sqs::SqsStoreError> {
        sqlx::query!(
            "INSERT INTO sqs_queues (name, region, account_id, queue_type, visibility_timeout_seconds, delay_seconds, message_retention_period_seconds, receive_message_wait_time_seconds) VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
            queue.name,
            queue.region,
            queue.account_id,
            queue.queue_type,
            queue.visibility_timeout_seconds,
            queue.delay_seconds,
            queue.message_retention_period_seconds,
            queue.receive_message_wait_time_seconds
        )
        .execute(&self.pool)
        .await
        .map_err(|err| hiraeth_store::sqs::SqsStoreError::StorageError(hiraeth_store::StorageError::StorageFailure(err.to_string())))?;
        Ok(())
    }

    async fn get_queue(
        &self,
        queue_name: &str,
        region: &str,
    ) -> Result<Option<SqsQueue>, SqsStoreError> {
        let queue = sqlx::query_as!(
            SqsQueue,
            "SELECT name, region, account_id, queue_type, visibility_timeout_seconds, delay_seconds, message_retention_period_seconds, receive_message_wait_time_seconds FROM sqs_queues WHERE name = ? AND region = ?",
            queue_name,
            region
        )
        .fetch_optional(&self.pool)
        .await
        .map_err(|err| SqsStoreError::StorageError(hiraeth_store::StorageError::StorageFailure(err.to_string())))?;
        Ok(queue)
    }
}
