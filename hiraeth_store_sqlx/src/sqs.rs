use ::chrono::Duration;
use async_trait::async_trait;
use hiraeth_store::StoreError;
use hiraeth_store::sqs::{SqsMessage, SqsQueue, SqsStore};
use sqlx::types::chrono;

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
    ) -> Result<(), hiraeth_store::StoreError> {
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
        .map_err(|err| hiraeth_store::StoreError::StorageFailure(err.to_string()))?;
        Ok(())
    }

    async fn get_queue(
        &self,
        queue_name: &str,
        region: &str,
        account_id: &str,
    ) -> Result<Option<SqsQueue>, StoreError> {
        let queue = sqlx::query_as!(
            SqsQueue,
            "SELECT id, name, region, account_id, queue_type, visibility_timeout_seconds, delay_seconds, message_retention_period_seconds, receive_message_wait_time_seconds
            FROM sqs_queues 
            WHERE name = ? AND region = ? AND account_id = ?",
            queue_name,
            region,
            account_id
        )
        .fetch_optional(&self.pool)
        .await
        .map_err(|err| StoreError::StorageFailure(err.to_string()))?;
        Ok(queue)
    }

    async fn send_message(&self, message: &SqsMessage) -> Result<(), StoreError> {
        sqlx::query!(
            "INSERT INTO sqs_messages (message_id, queue_id, body, message_attributes, aws_trace_header, sent_at, visible_at, expires_at, receive_count, receipt_handle, first_received_at, message_group_id, message_deduplication_id)
            VALUES (?,?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
            message.message_id,
            message.queue_id,
            message.body,
            message.message_attributes,
            message.aws_trace_header,
            message.sent_at,
            message.visible_at,
            message.expires_at,
            message.receive_count,
            message.receipt_handle,
            message.first_received_at,
            message.message_group_id,
            message.message_deduplication_id
        )
        .execute(&self.pool)
        .await
        .map_err(|err| StoreError::StorageFailure(err.to_string()))?;
        Ok(())
    }

    async fn receive_messages(
        &self,
        queue_id: i64,
        max_number_of_messages: i64,
        visibility_timeout_seconds: u32,
    ) -> Result<Vec<SqsMessage>, StoreError> {
        let mut conn = self
            .pool
            .acquire()
            .await
            .map_err(|err| StoreError::StorageFailure(err.to_string()))?;

        let now = chrono::Utc::now().naive_utc();

        let leased_until = chrono::Utc::now()
            .naive_utc()
            .checked_add_signed(Duration::seconds(visibility_timeout_seconds as i64))
            .ok_or_else(|| {
                StoreError::StorageFailure("failed to calculate leased_until".to_string())
            })?;

        sqlx::query!("BEGIN IMMEDIATE")
            .execute(&mut *conn)
            .await
            .map_err(|err| StoreError::StorageFailure(err.to_string()))?;

        let messages = sqlx::query_as!(
            SqsMessage,
            "WITH selected AS (
                SELECT id FROM sqs_messages
                WHERE queue_id = ? AND visible_at <= ? AND expires_at > ?
                ORDER BY sent_at, id
                LIMIT ?
            )
            UPDATE sqs_messages
            SET visible_at = ?, receipt_handle = lower(hex(randomblob(16))), receive_count = receive_count + 1, first_received_at = COALESCE(first_received_at, ?)
            WHERE id IN (SELECT id FROM selected)
            RETURNING
                message_id,
                queue_id,
                body,
                message_attributes,
                aws_trace_header,
                sent_at,
                visible_at,
                expires_at,
                receive_count,
                receipt_handle,
                first_received_at,
                message_group_id,
                message_deduplication_id
             ",
             queue_id,
                now,
                now,
                max_number_of_messages,
                leased_until,
                 now
        )
            .fetch_all(&mut *conn)
            .await
            .map_err(|err| StoreError::StorageFailure(err.to_string()))?;

        sqlx::query!("COMMIT")
            .execute(&mut *conn)
            .await
            .map_err(|err| {
                StoreError::StorageFailure(format!("failed to commit transaction: {}", err))
            })?;

        Ok(messages)
    }
}

#[cfg(test)]
mod tests {
    use chrono::{Duration, Utc};
    use tempfile::TempDir;

    use super::SqliteSqsStore;
    use crate::{get_store_pool, run_migrations};
    use hiraeth_store::sqs::{SqsMessage, SqsQueue, SqsStore};

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
            id: 0,
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

    fn message(
        queue_id: i64,
        message_id: &str,
        sent_at: chrono::NaiveDateTime,
        visible_at: chrono::NaiveDateTime,
        expires_at: chrono::NaiveDateTime,
    ) -> SqsMessage {
        SqsMessage {
            message_id: message_id.to_string(),
            queue_id,
            body: format!("body-{message_id}"),
            message_attributes: Some(
                r#"{"trace_id":{"DataType":"String","StringValue":"abc123","BinaryValue":null}}"#
                    .to_string(),
            ),
            aws_trace_header: None,
            sent_at,
            visible_at,
            expires_at,
            receive_count: 0,
            receipt_handle: None,
            first_received_at: None,
            message_group_id: None,
            message_deduplication_id: None,
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
            .get_queue("orders", "us-east-1", "123456789012")
            .await
            .expect("queue lookup should succeed")
            .expect("queue should exist");

        assert!(queue.id > 0);
        assert_eq!(queue.name, expected_queue.name);
        assert_eq!(queue.region, expected_queue.region);
        assert_eq!(queue.account_id, expected_queue.account_id);
        assert_eq!(queue.queue_type, expected_queue.queue_type);
        assert_eq!(
            queue.visibility_timeout_seconds,
            expected_queue.visibility_timeout_seconds
        );
        assert_eq!(queue.delay_seconds, expected_queue.delay_seconds);
        assert_eq!(
            queue.message_retention_period_seconds,
            expected_queue.message_retention_period_seconds
        );
        assert_eq!(
            queue.receive_message_wait_time_seconds,
            expected_queue.receive_message_wait_time_seconds
        );
    }

    #[tokio::test]
    async fn get_queue_returns_none_when_region_does_not_match() {
        let (_temp_dir, store) = test_store().await;

        store
            .create_queue(queue("orders", "us-east-1"))
            .await
            .expect("queue insert should succeed");

        let queue = store
            .get_queue("orders", "us-west-2", "123456789012")
            .await
            .expect("queue lookup should succeed");

        assert!(queue.is_none());
    }

    #[tokio::test]
    async fn get_queue_returns_none_for_missing_queue() {
        let (_temp_dir, store) = test_store().await;

        let queue = store
            .get_queue("missing", "us-east-1", "123456789012")
            .await
            .expect("queue lookup should succeed");

        assert!(queue.is_none());
    }

    #[tokio::test]
    async fn receive_messages_claims_visible_messages_and_skips_hidden_or_expired_messages() {
        let (_temp_dir, store) = test_store().await;

        store
            .create_queue(queue("orders", "us-east-1"))
            .await
            .expect("queue insert should succeed");

        let queue = store
            .get_queue("orders", "us-east-1", "123456789012")
            .await
            .expect("queue lookup should succeed")
            .expect("queue should exist");

        let now = Utc::now().naive_utc();

        store
            .send_message(&message(
                queue.id,
                "msg-1",
                now - Duration::seconds(20),
                now - Duration::seconds(10),
                now + Duration::hours(1),
            ))
            .await
            .expect("first visible message should insert");

        store
            .send_message(&message(
                queue.id,
                "msg-2",
                now - Duration::seconds(15),
                now - Duration::seconds(5),
                now + Duration::hours(1),
            ))
            .await
            .expect("second visible message should insert");

        store
            .send_message(&message(
                queue.id,
                "msg-hidden",
                now - Duration::seconds(5),
                now + Duration::minutes(5),
                now + Duration::hours(1),
            ))
            .await
            .expect("hidden message should insert");

        store
            .send_message(&message(
                queue.id,
                "msg-expired",
                now - Duration::seconds(30),
                now - Duration::seconds(25),
                now - Duration::seconds(1),
            ))
            .await
            .expect("expired message should insert");

        let received = store
            .receive_messages(queue.id, 10, 45)
            .await
            .expect("receive messages should succeed");

        assert_eq!(received.len(), 2);
        assert_eq!(received[0].message_id, "msg-1");
        assert_eq!(received[1].message_id, "msg-2");

        for message in &received {
            assert_eq!(message.receive_count, 1);
            assert!(message.receipt_handle.is_some());
            assert!(message.first_received_at.is_some());
            assert!(message.visible_at > now + Duration::seconds(40));
        }

        let received_again = store
            .receive_messages(queue.id, 10, 45)
            .await
            .expect("second receive should succeed");

        assert!(received_again.is_empty());
    }

    #[tokio::test]
    async fn receive_messages_preserves_first_received_at_and_increments_receive_count() {
        let (_temp_dir, store) = test_store().await;

        store
            .create_queue(queue("orders", "us-east-1"))
            .await
            .expect("queue insert should succeed");

        let queue = store
            .get_queue("orders", "us-east-1", "123456789012")
            .await
            .expect("queue lookup should succeed")
            .expect("queue should exist");

        let now = Utc::now().naive_utc();
        let original_first_received_at = now - Duration::minutes(2);

        store
            .send_message(&SqsMessage {
                receive_count: 3,
                receipt_handle: Some("old-receipt".to_string()),
                first_received_at: Some(original_first_received_at),
                aws_trace_header: Some("Root=1-abcdef12-0123456789abcdef01234567".to_string()),
                ..message(
                    queue.id,
                    "msg-received-before",
                    now - Duration::minutes(5),
                    now - Duration::seconds(5),
                    now + Duration::hours(1),
                )
            })
            .await
            .expect("message should insert");

        let received = store
            .receive_messages(queue.id, 1, 30)
            .await
            .expect("receive messages should succeed");

        assert_eq!(received.len(), 1);
        assert_eq!(received[0].message_id, "msg-received-before");
        assert_eq!(received[0].receive_count, 4);
        assert_eq!(
            received[0].first_received_at,
            Some(original_first_received_at)
        );
        assert_ne!(received[0].receipt_handle.as_deref(), Some("old-receipt"));
        assert_eq!(
            received[0].aws_trace_header.as_deref(),
            Some("Root=1-abcdef12-0123456789abcdef01234567")
        );
    }
}
