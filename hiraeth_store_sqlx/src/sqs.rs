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

#[cfg(test)]
mod tests {
    use tempfile::TempDir;

    use super::SqliteSqsStore;
    use crate::{get_store_pool, run_migrations};
    use hiraeth_store::sqs::{SqsQueue, SqsStore};

    async fn test_store() -> (TempDir, SqliteSqsStore) {
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

        (temp_dir, SqliteSqsStore::new(&pool))
    }

    fn queue(name: &str, region: &str) -> SqsQueue {
        SqsQueue {
            name: name.to_string(),
            region: region.to_string(),
            account_id: "123456789012".to_string(),
            queue_type: "standard".to_string(),
            visibility_timeout_seconds: 30,
            delay_seconds: 0,
            message_retention_period_seconds: 345600,
            receive_message_wait_time_seconds: 0,
        }
    }

    #[tokio::test]
    async fn run_migrations_creates_sqs_queues_table() {
        let (_temp_dir, store) = test_store().await;

        let table_name = sqlx::query_scalar::<_, String>(
            "SELECT name FROM sqlite_master WHERE type = 'table' AND name = 'sqs_queues'",
        )
        .fetch_one(&store.pool)
        .await
        .expect("sqs_queues table should exist after migrations");

        assert_eq!(table_name, "sqs_queues");
    }

    #[tokio::test]
    async fn create_queue_and_get_queue_round_trip() {
        let (_temp_dir, store) = test_store().await;
        let expected_queue = queue("orders", "us-east-1");

        store
            .create_queue(expected_queue.clone())
            .await
            .expect("queue insert should succeed");

        let queue = store
            .get_queue("orders", "us-east-1")
            .await
            .expect("queue lookup should succeed")
            .expect("queue should exist");

        assert_eq!(queue, expected_queue);
    }

    #[tokio::test]
    async fn get_queue_returns_none_when_region_does_not_match() {
        let (_temp_dir, store) = test_store().await;

        store
            .create_queue(queue("orders", "us-east-1"))
            .await
            .expect("queue insert should succeed");

        let queue = store
            .get_queue("orders", "us-west-2")
            .await
            .expect("queue lookup should succeed");

        assert!(queue.is_none());
    }

    #[tokio::test]
    async fn get_queue_returns_none_for_missing_queue() {
        let (_temp_dir, store) = test_store().await;

        let queue = store
            .get_queue("missing", "us-east-1")
            .await
            .expect("queue lookup should succeed");

        assert!(queue.is_none());
    }
}
