use std::collections::HashMap;

use ::chrono::Duration;
use async_trait::async_trait;
use hiraeth_store::StoreError;
use hiraeth_store::sqs::{SqsMessage, SqsQueue, SqsQueueAttributeUpdate, SqsStore};
use sqlx::types::chrono;

#[derive(Clone)]
pub struct SqliteSqsStore {
    pool: sqlx::SqlitePool,
}

impl SqliteSqsStore {
    pub fn new(pool: &sqlx::SqlitePool) -> Self {
        Self { pool: pool.clone() }
    }

    pub async fn get_queue_by_id(&self, queue_id: i64) -> Result<Option<SqsQueue>, StoreError> {
        let queue = sqlx::query_as!(
            SqsQueue,
            "SELECT
                id,
                name,
                region,
                account_id,
                queue_type,
                visibility_timeout_seconds,
                delay_seconds,
                maximum_message_size,
                message_retention_period_seconds,
                receive_message_wait_time_seconds,
                policy,
                redrive_policy,
                content_based_deduplication as \"content_based_deduplication: bool\",
                kms_master_key_id,
                kms_data_key_reuse_period_seconds,
                deduplication_scope,
                fifo_throughput_limit,
                redrive_allow_policy,
                sqs_managed_sse_enabled as \"sqs_managed_sse_enabled: bool\",
                created_at,
                updated_at
            FROM sqs_queues
            WHERE id = ?",
            queue_id
        )
        .fetch_optional(&self.pool)
        .await
        .map_err(map_sqlx_error)?;

        Ok(queue)
    }

    pub async fn list_messages(
        &self,
        queue_id: i64,
        limit: i64,
    ) -> Result<Vec<SqsMessage>, StoreError> {
        let messages = sqlx::query_as!(
            SqsMessage,
            "SELECT
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
            FROM sqs_messages
            WHERE queue_id = ?
            ORDER BY sent_at DESC, message_id
            LIMIT ?",
            queue_id,
            limit
        )
        .fetch_all(&self.pool)
        .await
        .map_err(map_sqlx_error)?;

        Ok(messages)
    }

    pub async fn delete_message_by_id(
        &self,
        queue_id: i64,
        message_id: &str,
    ) -> Result<(), StoreError> {
        let result = sqlx::query!(
            "DELETE FROM sqs_messages WHERE queue_id = ? AND message_id = ?",
            queue_id,
            message_id
        )
        .execute(&self.pool)
        .await
        .map_err(map_sqlx_error)?;

        if result.rows_affected() == 0 {
            return Err(StoreError::NotFound("message not found".to_string()));
        }

        Ok(())
    }

    async fn ensure_queue_exists(&self, queue_id: i64) -> Result<(), StoreError> {
        let queue_count =
            sqlx::query_scalar!("SELECT COUNT(*) FROM sqs_queues WHERE id = ?", queue_id)
                .fetch_one(&self.pool)
                .await
                .map_err(map_sqlx_error)?;

        if queue_count == 0 {
            return Err(StoreError::NotFound("queue not found".to_string()));
        }

        Ok(())
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
impl SqsStore for SqliteSqsStore {
    async fn create_queue(&self, queue: SqsQueue) -> Result<(), StoreError> {
        sqlx::query!(
            "INSERT INTO sqs_queues (
                name,
                region,
                account_id,
                queue_type,
                visibility_timeout_seconds,
                delay_seconds,
                maximum_message_size,
                message_retention_period_seconds,
                receive_message_wait_time_seconds,
                policy,
                redrive_policy,
                content_based_deduplication,
                kms_master_key_id,
                kms_data_key_reuse_period_seconds,
                deduplication_scope,
                fifo_throughput_limit,
                redrive_allow_policy,
                sqs_managed_sse_enabled,
                updated_at
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
            queue.name,
            queue.region,
            queue.account_id,
            queue.queue_type,
            queue.visibility_timeout_seconds,
            queue.delay_seconds,
            queue.maximum_message_size,
            queue.message_retention_period_seconds,
            queue.receive_message_wait_time_seconds,
            queue.policy,
            queue.redrive_policy,
            queue.content_based_deduplication,
            queue.kms_master_key_id,
            queue.kms_data_key_reuse_period_seconds,
            queue.deduplication_scope,
            queue.fifo_throughput_limit,
            queue.redrive_allow_policy,
            queue.sqs_managed_sse_enabled,
            queue.updated_at
        )
        .execute(&self.pool)
        .await
        .map_err(map_sqlx_error)?;
        Ok(())
    }

    async fn delete_queue(&self, queue_id: i64) -> Result<(), StoreError> {
        let result = sqlx::query!("DELETE FROM sqs_queues WHERE id = ?", queue_id)
            .execute(&self.pool)
            .await
            .map_err(map_sqlx_error)?;
        if result.rows_affected() == 0 {
            return Err(StoreError::NotFound("queue not found".to_string()));
        }
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
            "SELECT
                id,
                name,
                region,
                account_id,
                queue_type,
                visibility_timeout_seconds,
                delay_seconds,
                maximum_message_size,
                message_retention_period_seconds,
                receive_message_wait_time_seconds,
                policy,
                redrive_policy,
                content_based_deduplication as \"content_based_deduplication: bool\",
                kms_master_key_id,
                kms_data_key_reuse_period_seconds,
                deduplication_scope,
                fifo_throughput_limit,
                redrive_allow_policy,
                sqs_managed_sse_enabled as \"sqs_managed_sse_enabled: bool\",
                created_at,
                updated_at
            FROM sqs_queues 
            WHERE name = ? AND region = ? AND account_id = ?",
            queue_name,
            region,
            account_id
        )
        .fetch_optional(&self.pool)
        .await
        .map_err(map_sqlx_error)?;
        Ok(queue)
    }

    async fn send_message(&self, message: &SqsMessage) -> Result<(), StoreError> {
        sqlx::query!(
            "INSERT INTO sqs_messages (message_id, queue_id, body, message_attributes, aws_trace_header, sent_at, visible_at, expires_at, receive_count, receipt_handle, first_received_at, message_group_id, message_deduplication_id)
            VALUES (?, ?, ?, COALESCE(?, '{}'), ?, ?, ?, ?, ?, ?, ?, ?, ?)",
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
        .map_err(map_sqlx_error)?;
        Ok(())
    }

    async fn receive_messages(
        &self,
        queue_id: i64,
        max_number_of_messages: i64,
        visibility_timeout_seconds: u32,
    ) -> Result<Vec<SqsMessage>, StoreError> {
        let mut conn = self.pool.acquire().await.map_err(map_sqlx_error)?;

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
            .map_err(map_sqlx_error)?;

        let messages = match sqlx::query_as!(
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
        {
            Ok(messages) => messages,
            Err(err) => {
                let _ = sqlx::query("ROLLBACK").execute(&mut *conn).await;
                return Err(map_sqlx_error(err));
            }
        };

        if let Err(err) = sqlx::query!("COMMIT").execute(&mut *conn).await {
            let _ = sqlx::query("ROLLBACK").execute(&mut *conn).await;
            return Err(StoreError::StorageFailure(format!(
                "failed to commit transaction: {}",
                err
            )));
        }

        Ok(messages)
    }

    async fn get_message_count(&self, queue_id: i64) -> Result<i64, StoreError> {
        let now = chrono::Utc::now().naive_utc();
        let count = sqlx::query_scalar!(
            "SELECT COUNT(*) FROM sqs_messages WHERE queue_id = ? AND expires_at > ?",
            queue_id,
            now
        )
        .fetch_one(&self.pool)
        .await
        .map_err(map_sqlx_error)?;
        Ok(count)
    }

    async fn get_visible_message_count(&self, queue_id: i64) -> Result<i64, StoreError> {
        let now = chrono::Utc::now().naive_utc();
        let count = sqlx::query_scalar!(
            "SELECT COUNT(*) FROM sqs_messages WHERE queue_id = ? AND visible_at <= ? AND expires_at > ?",
            queue_id,
            now,
            now
        )
        .fetch_one(&self.pool)
        .await
        .map_err(map_sqlx_error)?;
        Ok(count)
    }

    async fn get_messages_delayed_count(&self, queue_id: i64) -> Result<i64, StoreError> {
        let now = chrono::Utc::now().naive_utc();
        let count = sqlx::query_scalar!(
            "SELECT COUNT(*) FROM sqs_messages WHERE queue_id = ? AND visible_at > ? AND expires_at > ? AND receipt_handle IS NULL",
            queue_id,
            now,
            now
        )
        .fetch_one(&self.pool)
        .await
        .map_err(map_sqlx_error)?;
        Ok(count)
    }

    async fn delete_message(&self, queue_id: i64, receipt_handle: &str) -> Result<(), StoreError> {
        let result = sqlx::query!(
            "DELETE FROM sqs_messages WHERE queue_id = ? AND receipt_handle = ?",
            queue_id,
            receipt_handle
        )
        .execute(&self.pool)
        .await
        .map_err(map_sqlx_error)?;
        if result.rows_affected() == 0 {
            return Err(StoreError::NotFound("receipt handle not found".to_string()));
        }
        Ok(())
    }

    async fn set_message_visible_at(
        &self,
        queue_id: i64,
        receipt_handle: &str,
        visible_at: chrono::NaiveDateTime,
    ) -> Result<(), StoreError> {
        let result = sqlx::query!(
            "UPDATE sqs_messages SET visible_at = ? WHERE queue_id = ? AND receipt_handle = ?",
            visible_at,
            queue_id,
            receipt_handle
        )
        .execute(&self.pool)
        .await
        .map_err(map_sqlx_error)?;
        if result.rows_affected() == 0 {
            return Err(StoreError::NotFound("receipt handle not found".to_string()));
        }
        Ok(())
    }

    async fn list_queues(
        &self,
        region: &str,
        account_id: &str,
        queue_name_prefix: Option<&str>,
        max_results: Option<i64>,
        next_token: Option<&str>,
    ) -> Result<Vec<SqsQueue>, StoreError> {
        let limit = max_results.unwrap_or(1000);
        let result = sqlx::query_as!(
            SqsQueue,
            "SELECT
                id,
                name,
                region,
                account_id,
                queue_type,
                visibility_timeout_seconds,
                delay_seconds,
                maximum_message_size,
                message_retention_period_seconds,
                receive_message_wait_time_seconds,
                policy,
                redrive_policy,
                content_based_deduplication as \"content_based_deduplication: bool\",
                kms_master_key_id,
                kms_data_key_reuse_period_seconds,
                deduplication_scope,
                fifo_throughput_limit,
                redrive_allow_policy,
                sqs_managed_sse_enabled as \"sqs_managed_sse_enabled: bool\",
                created_at,
                updated_at
            FROM sqs_queues
            WHERE region = ?
            AND account_id = ?
            AND (? IS NULL OR substr(name, 1, length(?)) = ?)
            AND (? IS NULL OR name > ?)
            ORDER BY name
            LIMIT ?",
            region,
            account_id,
            queue_name_prefix,
            queue_name_prefix,
            queue_name_prefix,
            next_token,
            next_token,
            limit
        )
        .fetch_all(&self.pool)
        .await
        .map_err(map_sqlx_error)?;

        Ok(result)
    }

    async fn purge_queue(&self, queue_id: i64) -> Result<(), StoreError> {
        sqlx::query!("DELETE FROM sqs_messages WHERE queue_id = ?", queue_id)
            .execute(&self.pool)
            .await
            .map_err(map_sqlx_error)?;
        Ok(())
    }

    async fn set_queue_attributes(
        &self,
        queue_id: i64,
        attributes: SqsQueueAttributeUpdate,
    ) -> Result<(), StoreError> {
        let updated_at = chrono::Utc::now().naive_utc();
        let update_kms_master_key_id = attributes.kms_master_key_id.is_some();
        let kms_master_key_id = attributes.kms_master_key_id.flatten();

        let result = sqlx::query!(
            "UPDATE sqs_queues SET
                visibility_timeout_seconds = COALESCE(?, visibility_timeout_seconds),
                delay_seconds = COALESCE(?, delay_seconds),
                maximum_message_size = COALESCE(?, maximum_message_size),
                message_retention_period_seconds = COALESCE(?, message_retention_period_seconds),
                receive_message_wait_time_seconds = COALESCE(?, receive_message_wait_time_seconds),
                policy = COALESCE(?, policy),
                redrive_policy = COALESCE(?, redrive_policy),
                content_based_deduplication = COALESCE(?, content_based_deduplication),
                kms_master_key_id = CASE WHEN ? THEN ? ELSE kms_master_key_id END,
                kms_data_key_reuse_period_seconds = COALESCE(?, kms_data_key_reuse_period_seconds),
                deduplication_scope = COALESCE(?, deduplication_scope),
                fifo_throughput_limit = COALESCE(?, fifo_throughput_limit),
                redrive_allow_policy = COALESCE(?, redrive_allow_policy),
                sqs_managed_sse_enabled = COALESCE(?, sqs_managed_sse_enabled),
                updated_at = ?
            WHERE id = ?",
            attributes.visibility_timeout_seconds,
            attributes.delay_seconds,
            attributes.maximum_message_size,
            attributes.message_retention_period_seconds,
            attributes.receive_message_wait_time_seconds,
            attributes.policy,
            attributes.redrive_policy,
            attributes.content_based_deduplication,
            update_kms_master_key_id,
            kms_master_key_id,
            attributes.kms_data_key_reuse_period_seconds,
            attributes.deduplication_scope,
            attributes.fifo_throughput_limit,
            attributes.redrive_allow_policy,
            attributes.sqs_managed_sse_enabled,
            updated_at,
            queue_id
        )
        .execute(&self.pool)
        .await
        .map_err(map_sqlx_error)?;
        if result.rows_affected() == 0 {
            return Err(StoreError::NotFound("queue not found".to_string()));
        }

        Ok(())
    }

    async fn list_queue_tags(&self, queue_id: i64) -> Result<HashMap<String, String>, StoreError> {
        self.ensure_queue_exists(queue_id).await?;

        let rows = sqlx::query!(
            "SELECT tag_key, tag_value FROM sqs_queue_tags WHERE queue_id = ? ORDER BY tag_key",
            queue_id
        )
        .fetch_all(&self.pool)
        .await
        .map_err(map_sqlx_error)?;

        Ok(rows
            .into_iter()
            .map(|row| (row.tag_key, row.tag_value))
            .collect())
    }

    async fn tag_queue(
        &self,
        queue_id: i64,
        tags: HashMap<String, String>,
    ) -> Result<(), StoreError> {
        self.ensure_queue_exists(queue_id).await?;

        for (key, value) in tags {
            sqlx::query!(
                "INSERT INTO sqs_queue_tags (queue_id, tag_key, tag_value)
                VALUES (?, ?, ?)
                ON CONFLICT(queue_id, tag_key) DO UPDATE SET tag_value = excluded.tag_value",
                queue_id,
                key,
                value
            )
            .execute(&self.pool)
            .await
            .map_err(map_sqlx_error)?;
        }

        Ok(())
    }

    async fn untag_queue(&self, queue_id: i64, tag_keys: Vec<String>) -> Result<(), StoreError> {
        self.ensure_queue_exists(queue_id).await?;

        for key in tag_keys {
            sqlx::query!(
                "DELETE FROM sqs_queue_tags WHERE queue_id = ? AND tag_key = ?",
                queue_id,
                key
            )
            .execute(&self.pool)
            .await
            .map_err(map_sqlx_error)?;
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use chrono::{Duration, Utc};
    use tempfile::TempDir;

    use super::SqliteSqsStore;
    use crate::{get_store_pool, run_migrations};
    use hiraeth_store::{
        StoreError,
        sqs::{SqsMessage, SqsQueue, SqsQueueAttributeUpdate, SqsStore},
    };

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
        queue_for_account(name, region, "123456789012")
    }

    fn queue_for_account(name: &str, region: &str, account_id: &str) -> SqsQueue {
        let now = Utc::now().naive_utc();

        SqsQueue {
            id: 0,
            name: name.to_string(),
            region: region.to_string(),
            account_id: account_id.to_string(),
            queue_type: "standard".to_string(),
            visibility_timeout_seconds: 30,
            delay_seconds: 0,
            message_retention_period_seconds: 345600,
            receive_message_wait_time_seconds: 0,
            created_at: now,
            updated_at: now,
            ..Default::default()
        }
    }

    fn queue_names(queues: Vec<SqsQueue>) -> Vec<String> {
        queues.into_iter().map(|queue| queue.name).collect()
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
    async fn run_migrations_creates_sqs_queue_tags_table() {
        let (_temp_dir, store) = test_store().await;

        let table_name = sqlx::query_scalar::<_, String>(
            "SELECT name FROM sqlite_master WHERE type = 'table' AND name = 'sqs_queue_tags'",
        )
        .fetch_one(&store.pool)
        .await
        .expect("sqs_queue_tags table should exist after migrations");

        assert_eq!(table_name, "sqs_queue_tags");
    }

    #[tokio::test]
    async fn create_queue_and_get_queue_round_trip() {
        let (_temp_dir, store) = test_store().await;
        let expected_queue = SqsQueue {
            queue_type: "fifo".to_string(),
            visibility_timeout_seconds: 45,
            delay_seconds: 5,
            maximum_message_size: 2048,
            message_retention_period_seconds: 86400,
            receive_message_wait_time_seconds: 10,
            policy: r#"{"Statement":[]}"#.to_string(),
            redrive_policy: r#"{"maxReceiveCount":"5"}"#.to_string(),
            content_based_deduplication: true,
            kms_master_key_id: Some("alias/test".to_string()),
            kms_data_key_reuse_period_seconds: 600,
            deduplication_scope: "messageGroup".to_string(),
            fifo_throughput_limit: "perMessageGroupId".to_string(),
            redrive_allow_policy: r#"{"redrivePermission":"allowAll"}"#.to_string(),
            sqs_managed_sse_enabled: true,
            ..queue("orders.fifo", "us-east-1")
        };

        store
            .create_queue(expected_queue.clone())
            .await
            .expect("queue insert should succeed");

        let queue = store
            .get_queue("orders.fifo", "us-east-1", "123456789012")
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
        assert_eq!(
            queue.maximum_message_size,
            expected_queue.maximum_message_size
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
        assert_eq!(queue.policy, expected_queue.policy);
        assert_eq!(queue.redrive_policy, expected_queue.redrive_policy);
        assert_eq!(
            queue.content_based_deduplication,
            expected_queue.content_based_deduplication
        );
        assert_eq!(queue.kms_master_key_id, expected_queue.kms_master_key_id);
        assert_eq!(
            queue.kms_data_key_reuse_period_seconds,
            expected_queue.kms_data_key_reuse_period_seconds
        );
        assert_eq!(
            queue.deduplication_scope,
            expected_queue.deduplication_scope
        );
        assert_eq!(
            queue.fifo_throughput_limit,
            expected_queue.fifo_throughput_limit
        );
        assert_eq!(
            queue.redrive_allow_policy,
            expected_queue.redrive_allow_policy
        );
        assert_eq!(
            queue.sqs_managed_sse_enabled,
            expected_queue.sqs_managed_sse_enabled
        );
        assert_eq!(queue.updated_at, expected_queue.updated_at);
    }

    #[tokio::test]
    async fn get_queue_by_id_returns_matching_queue() {
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

        let queue_by_id = store
            .get_queue_by_id(queue.id)
            .await
            .expect("queue lookup by id should succeed")
            .expect("queue should exist");

        assert_eq!(queue_by_id, queue);
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
    async fn delete_queue_removes_queue() {
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

        store
            .delete_queue(queue.id)
            .await
            .expect("queue delete should succeed");

        let queue = store
            .get_queue("orders", "us-east-1", "123456789012")
            .await
            .expect("queue lookup should succeed");

        assert!(queue.is_none());
    }

    #[tokio::test]
    async fn set_queue_attributes_updates_only_matching_queue() {
        let (_temp_dir, store) = test_store().await;
        let original_updated_at = Utc::now().naive_utc() - Duration::days(1);

        store
            .create_queue(SqsQueue {
                updated_at: original_updated_at,
                kms_master_key_id: Some("alias/original".to_string()),
                ..queue("orders", "us-east-1")
            })
            .await
            .expect("orders queue should insert");
        store
            .create_queue(queue("payments", "us-east-1"))
            .await
            .expect("payments queue should insert");

        let orders = store
            .get_queue("orders", "us-east-1", "123456789012")
            .await
            .expect("orders lookup should succeed")
            .expect("orders queue should exist");

        store
            .set_queue_attributes(
                orders.id,
                SqsQueueAttributeUpdate {
                    visibility_timeout_seconds: Some(45),
                    delay_seconds: Some(5),
                    maximum_message_size: Some(2048),
                    message_retention_period_seconds: Some(86400),
                    receive_message_wait_time_seconds: Some(10),
                    policy: Some(r#"{"Statement":[]}"#.to_string()),
                    redrive_policy: Some(r#"{"maxReceiveCount":"5"}"#.to_string()),
                    content_based_deduplication: Some(true),
                    kms_master_key_id: Some(Some("alias/updated".to_string())),
                    kms_data_key_reuse_period_seconds: Some(600),
                    deduplication_scope: Some("messageGroup".to_string()),
                    fifo_throughput_limit: Some("perMessageGroupId".to_string()),
                    redrive_allow_policy: Some(r#"{"redrivePermission":"allowAll"}"#.to_string()),
                    sqs_managed_sse_enabled: Some(true),
                },
            )
            .await
            .expect("set queue attributes should succeed");

        let updated_orders = store
            .get_queue("orders", "us-east-1", "123456789012")
            .await
            .expect("orders lookup should succeed")
            .expect("orders queue should exist");
        let payments = store
            .get_queue("payments", "us-east-1", "123456789012")
            .await
            .expect("payments lookup should succeed")
            .expect("payments queue should exist");

        assert_eq!(updated_orders.visibility_timeout_seconds, 45);
        assert_eq!(updated_orders.delay_seconds, 5);
        assert_eq!(updated_orders.maximum_message_size, 2048);
        assert_eq!(updated_orders.message_retention_period_seconds, 86400);
        assert_eq!(updated_orders.receive_message_wait_time_seconds, 10);
        assert_eq!(updated_orders.policy, r#"{"Statement":[]}"#);
        assert_eq!(updated_orders.redrive_policy, r#"{"maxReceiveCount":"5"}"#);
        assert!(updated_orders.content_based_deduplication);
        assert_eq!(
            updated_orders.kms_master_key_id.as_deref(),
            Some("alias/updated")
        );
        assert_eq!(updated_orders.kms_data_key_reuse_period_seconds, 600);
        assert_eq!(updated_orders.deduplication_scope, "messageGroup");
        assert_eq!(updated_orders.fifo_throughput_limit, "perMessageGroupId");
        assert_eq!(
            updated_orders.redrive_allow_policy,
            r#"{"redrivePermission":"allowAll"}"#
        );
        assert!(updated_orders.sqs_managed_sse_enabled);
        assert!(updated_orders.updated_at > original_updated_at);

        assert_eq!(payments.visibility_timeout_seconds, 30);
        assert_eq!(payments.delay_seconds, 0);
        assert_eq!(payments.kms_master_key_id, None);
        assert!(!payments.sqs_managed_sse_enabled);
    }

    #[tokio::test]
    async fn set_queue_attributes_preserves_omitted_values() {
        let (_temp_dir, store) = test_store().await;
        store
            .create_queue(SqsQueue {
                delay_seconds: 9,
                policy: r#"{"Statement":[]}"#.to_string(),
                kms_master_key_id: Some("alias/original".to_string()),
                ..queue("orders", "us-east-1")
            })
            .await
            .expect("queue should insert");

        let queue = store
            .get_queue("orders", "us-east-1", "123456789012")
            .await
            .expect("queue lookup should succeed")
            .expect("queue should exist");

        store
            .set_queue_attributes(
                queue.id,
                SqsQueueAttributeUpdate {
                    visibility_timeout_seconds: Some(45),
                    ..SqsQueueAttributeUpdate::default()
                },
            )
            .await
            .expect("set queue attributes should succeed");

        let updated_queue = store
            .get_queue("orders", "us-east-1", "123456789012")
            .await
            .expect("queue lookup should succeed")
            .expect("queue should exist");

        assert_eq!(updated_queue.visibility_timeout_seconds, 45);
        assert_eq!(updated_queue.delay_seconds, 9);
        assert_eq!(updated_queue.policy, r#"{"Statement":[]}"#);
        assert_eq!(
            updated_queue.kms_master_key_id.as_deref(),
            Some("alias/original")
        );

        store
            .set_queue_attributes(
                queue.id,
                SqsQueueAttributeUpdate {
                    kms_master_key_id: Some(None),
                    ..SqsQueueAttributeUpdate::default()
                },
            )
            .await
            .expect("clearing kms master key id should succeed");

        let updated_queue = store
            .get_queue("orders", "us-east-1", "123456789012")
            .await
            .expect("queue lookup should succeed")
            .expect("queue should exist");

        assert_eq!(updated_queue.kms_master_key_id, None);
    }

    #[tokio::test]
    async fn tag_queue_upserts_and_list_queue_tags_returns_tags() {
        let (_temp_dir, store) = test_store().await;
        store
            .create_queue(queue("orders", "us-east-1"))
            .await
            .expect("queue should insert");
        let queue = store
            .get_queue("orders", "us-east-1", "123456789012")
            .await
            .expect("queue lookup should succeed")
            .expect("queue should exist");

        store
            .tag_queue(
                queue.id,
                [
                    ("environment".to_string(), "test".to_string()),
                    ("owner".to_string(), "old".to_string()),
                ]
                .into_iter()
                .collect(),
            )
            .await
            .expect("initial tags should insert");
        store
            .tag_queue(
                queue.id,
                [("owner".to_string(), "hiraeth".to_string())]
                    .into_iter()
                    .collect(),
            )
            .await
            .expect("existing tag should update");

        let tags = store
            .list_queue_tags(queue.id)
            .await
            .expect("tags should list");

        assert_eq!(
            tags,
            [
                ("environment".to_string(), "test".to_string()),
                ("owner".to_string(), "hiraeth".to_string()),
            ]
            .into_iter()
            .collect::<HashMap<_, _>>()
        );
    }

    #[tokio::test]
    async fn untag_queue_removes_requested_tags() {
        let (_temp_dir, store) = test_store().await;
        store
            .create_queue(queue("orders", "us-east-1"))
            .await
            .expect("queue should insert");
        let queue = store
            .get_queue("orders", "us-east-1", "123456789012")
            .await
            .expect("queue lookup should succeed")
            .expect("queue should exist");
        store
            .tag_queue(
                queue.id,
                [
                    ("environment".to_string(), "test".to_string()),
                    ("owner".to_string(), "hiraeth".to_string()),
                ]
                .into_iter()
                .collect(),
            )
            .await
            .expect("tags should insert");

        store
            .untag_queue(queue.id, vec!["owner".to_string(), "missing".to_string()])
            .await
            .expect("tags should delete idempotently");

        let tags = store
            .list_queue_tags(queue.id)
            .await
            .expect("tags should list");

        assert_eq!(
            tags,
            [("environment".to_string(), "test".to_string())]
                .into_iter()
                .collect::<HashMap<_, _>>()
        );
    }

    #[tokio::test]
    async fn delete_queue_cascades_queue_tags() {
        let (_temp_dir, store) = test_store().await;
        store
            .create_queue(queue("orders", "us-east-1"))
            .await
            .expect("queue should insert");
        let queue = store
            .get_queue("orders", "us-east-1", "123456789012")
            .await
            .expect("queue lookup should succeed")
            .expect("queue should exist");
        store
            .tag_queue(
                queue.id,
                [("environment".to_string(), "test".to_string())]
                    .into_iter()
                    .collect(),
            )
            .await
            .expect("tags should insert");

        store
            .delete_queue(queue.id)
            .await
            .expect("queue should delete");

        let tag_count =
            sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM sqs_queue_tags WHERE queue_id = ?")
                .bind(queue.id)
                .fetch_one(&store.pool)
                .await
                .expect("tag count query should succeed");

        assert_eq!(tag_count, 0);
    }

    #[tokio::test]
    async fn queue_tag_methods_return_not_found_for_missing_queue() {
        let (_temp_dir, store) = test_store().await;

        let list_result = store.list_queue_tags(999).await;
        let tag_result = store
            .tag_queue(
                999,
                [("environment".to_string(), "test".to_string())]
                    .into_iter()
                    .collect(),
            )
            .await;
        let untag_result = store
            .untag_queue(999, vec!["environment".to_string()])
            .await;

        assert!(matches!(list_result, Err(StoreError::NotFound(_))));
        assert!(matches!(tag_result, Err(StoreError::NotFound(_))));
        assert!(matches!(untag_result, Err(StoreError::NotFound(_))));
    }

    #[tokio::test]
    async fn list_queues_filters_by_region_account_and_prefix() {
        let (_temp_dir, store) = test_store().await;

        for queue in [
            queue("orders-dev", "us-east-1"),
            queue("orders-prod", "us-east-1"),
            queue("billing-prod", "us-east-1"),
            queue("orders-west", "us-west-2"),
            queue_for_account("orders-other-account", "us-east-1", "999999999999"),
        ] {
            store
                .create_queue(queue)
                .await
                .expect("queue insert should succeed");
        }

        let queues = store
            .list_queues("us-east-1", "123456789012", Some("orders-"), None, None)
            .await
            .expect("list queues should succeed");

        assert_eq!(queue_names(queues), vec!["orders-dev", "orders-prod"]);
    }

    #[tokio::test]
    async fn list_queues_limits_results_in_name_order() {
        let (_temp_dir, store) = test_store().await;

        for queue in [
            queue("zebra", "us-east-1"),
            queue("alpha", "us-east-1"),
            queue("middle", "us-east-1"),
        ] {
            store
                .create_queue(queue)
                .await
                .expect("queue insert should succeed");
        }

        let queues = store
            .list_queues("us-east-1", "123456789012", None, Some(2), None)
            .await
            .expect("list queues should succeed");

        assert_eq!(queue_names(queues), vec!["alpha", "middle"]);
    }

    #[tokio::test]
    async fn list_queues_uses_next_token_as_name_cursor() {
        let (_temp_dir, store) = test_store().await;

        for queue in [
            queue("orders-001", "us-east-1"),
            queue("orders-002", "us-east-1"),
            queue("orders-003", "us-east-1"),
            queue("payments-001", "us-east-1"),
        ] {
            store
                .create_queue(queue)
                .await
                .expect("queue insert should succeed");
        }

        let queues = store
            .list_queues(
                "us-east-1",
                "123456789012",
                Some("orders-"),
                Some(10),
                Some("orders-001"),
            )
            .await
            .expect("list queues should succeed");

        assert_eq!(queue_names(queues), vec!["orders-002", "orders-003"]);
    }

    #[tokio::test]
    async fn list_queues_treats_underscore_prefix_literally() {
        let (_temp_dir, store) = test_store().await;

        for queue in [
            queue("orders_dev", "us-east-1"),
            queue("orders-prod", "us-east-1"),
        ] {
            store
                .create_queue(queue)
                .await
                .expect("queue insert should succeed");
        }

        let queues = store
            .list_queues("us-east-1", "123456789012", Some("orders_"), None, None)
            .await
            .expect("list queues should succeed");

        assert_eq!(queue_names(queues), vec!["orders_dev"]);
    }

    #[tokio::test]
    async fn queue_message_statistics_return_expected_counts() {
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
                "msg-visible",
                now - Duration::seconds(20),
                now - Duration::seconds(10),
                now + Duration::hours(1),
            ))
            .await
            .expect("visible message should insert");

        store
            .send_message(&message(
                queue.id,
                "msg-delayed",
                now - Duration::seconds(10),
                now + Duration::minutes(5),
                now + Duration::hours(1),
            ))
            .await
            .expect("delayed message should insert");

        store
            .send_message(&SqsMessage {
                receipt_handle: Some("claimed".to_string()),
                ..message(
                    queue.id,
                    "msg-in-flight",
                    now - Duration::seconds(15),
                    now + Duration::minutes(2),
                    now + Duration::hours(1),
                )
            })
            .await
            .expect("in flight message should insert");

        store
            .send_message(&message(
                queue.id,
                "msg-expired",
                now - Duration::minutes(5),
                now - Duration::minutes(4),
                now - Duration::seconds(1),
            ))
            .await
            .expect("expired message should insert");

        assert_eq!(
            store
                .get_message_count(queue.id)
                .await
                .expect("message count should succeed"),
            3
        );
        assert_eq!(
            store
                .get_visible_message_count(queue.id)
                .await
                .expect("visible message count should succeed"),
            1
        );
        assert_eq!(
            store
                .get_messages_delayed_count(queue.id)
                .await
                .expect("delayed message count should succeed"),
            1
        );
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

    #[tokio::test]
    async fn delete_message_removes_only_matching_receipt_handle() {
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
            .send_message(&SqsMessage {
                receipt_handle: Some("receipt-1".to_string()),
                ..message(
                    queue.id,
                    "msg-1",
                    now - Duration::seconds(10),
                    now - Duration::seconds(10),
                    now + Duration::hours(1),
                )
            })
            .await
            .expect("first message should insert");

        store
            .send_message(&SqsMessage {
                receipt_handle: Some("receipt-2".to_string()),
                ..message(
                    queue.id,
                    "msg-2",
                    now - Duration::seconds(10),
                    now - Duration::seconds(10),
                    now + Duration::hours(1),
                )
            })
            .await
            .expect("second message should insert");

        store
            .delete_message(queue.id, "receipt-1")
            .await
            .expect("delete message should succeed");

        let remaining =
            sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM sqs_messages WHERE queue_id = ?")
                .bind(queue.id)
                .fetch_one(&store.pool)
                .await
                .expect("message count query should succeed");

        let remaining_message_id = sqlx::query_scalar::<_, String>(
            "SELECT message_id FROM sqs_messages WHERE queue_id = ?",
        )
        .bind(queue.id)
        .fetch_one(&store.pool)
        .await
        .expect("remaining message query should succeed");

        assert_eq!(remaining, 1);
        assert_eq!(remaining_message_id, "msg-2");
    }

    #[tokio::test]
    async fn list_messages_returns_recent_messages_in_descending_sent_order() {
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
        for (message_id, sent_at) in [
            ("msg-old", now - Duration::minutes(3)),
            ("msg-new", now - Duration::minutes(1)),
            ("msg-middle", now - Duration::minutes(2)),
        ] {
            store
                .send_message(&message(
                    queue.id,
                    message_id,
                    sent_at,
                    sent_at,
                    now + Duration::hours(1),
                ))
                .await
                .expect("message should insert");
        }

        let messages = store
            .list_messages(queue.id, 2)
            .await
            .expect("messages should list");

        assert_eq!(
            messages
                .iter()
                .map(|message| message.message_id.as_str())
                .collect::<Vec<_>>(),
            vec!["msg-new", "msg-middle"]
        );
    }

    #[tokio::test]
    async fn delete_message_by_id_removes_matching_message() {
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
                "msg-delete",
                now,
                now,
                now + Duration::hours(1),
            ))
            .await
            .expect("message should insert");

        store
            .delete_message_by_id(queue.id, "msg-delete")
            .await
            .expect("message should delete");

        let messages = store
            .list_messages(queue.id, 10)
            .await
            .expect("messages should list");

        assert!(messages.is_empty());
    }

    #[tokio::test]
    async fn delete_message_by_id_returns_not_found_for_missing_message() {
        let (_temp_dir, store) = test_store().await;

        let result = store.delete_message_by_id(1, "missing-message").await;

        assert!(matches!(result, Err(StoreError::NotFound(_))));
    }

    #[tokio::test]
    async fn delete_message_returns_not_found_for_missing_receipt_handle() {
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

        let result = store.delete_message(queue.id, "missing-receipt").await;

        assert!(matches!(result, Err(StoreError::NotFound(_))));
    }

    #[tokio::test]
    async fn purge_queue_removes_only_messages_for_matching_queue() {
        let (_temp_dir, store) = test_store().await;

        store
            .create_queue(queue("orders", "us-east-1"))
            .await
            .expect("orders queue insert should succeed");
        store
            .create_queue(queue("payments", "us-east-1"))
            .await
            .expect("payments queue insert should succeed");

        let orders_queue = store
            .get_queue("orders", "us-east-1", "123456789012")
            .await
            .expect("orders queue lookup should succeed")
            .expect("orders queue should exist");
        let payments_queue = store
            .get_queue("payments", "us-east-1", "123456789012")
            .await
            .expect("payments queue lookup should succeed")
            .expect("payments queue should exist");

        let now = Utc::now().naive_utc();
        for message in [
            message(
                orders_queue.id,
                "orders-1",
                now - Duration::seconds(20),
                now - Duration::seconds(10),
                now + Duration::hours(1),
            ),
            message(
                orders_queue.id,
                "orders-2",
                now - Duration::seconds(15),
                now + Duration::minutes(5),
                now + Duration::hours(1),
            ),
            message(
                payments_queue.id,
                "payments-1",
                now - Duration::seconds(10),
                now - Duration::seconds(5),
                now + Duration::hours(1),
            ),
        ] {
            store
                .send_message(&message)
                .await
                .expect("message insert should succeed");
        }

        store
            .purge_queue(orders_queue.id)
            .await
            .expect("purge queue should succeed");

        let orders_message_count =
            sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM sqs_messages WHERE queue_id = ?")
                .bind(orders_queue.id)
                .fetch_one(&store.pool)
                .await
                .expect("orders message count query should succeed");
        let payments_message_count =
            sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM sqs_messages WHERE queue_id = ?")
                .bind(payments_queue.id)
                .fetch_one(&store.pool)
                .await
                .expect("payments message count query should succeed");
        let orders_queue = store
            .get_queue("orders", "us-east-1", "123456789012")
            .await
            .expect("orders queue lookup should succeed");

        assert_eq!(orders_message_count, 0);
        assert_eq!(payments_message_count, 1);
        assert!(orders_queue.is_some());
    }

    #[tokio::test]
    async fn set_message_visible_at_updates_visibility_for_matching_receipt_handle() {
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
        let initial_visible_at = now - Duration::seconds(10);
        let updated_visible_at = now + Duration::minutes(3);

        store
            .send_message(&SqsMessage {
                receipt_handle: Some("receipt-1".to_string()),
                ..message(
                    queue.id,
                    "msg-1",
                    now - Duration::seconds(20),
                    initial_visible_at,
                    now + Duration::hours(1),
                )
            })
            .await
            .expect("message should insert");

        store
            .set_message_visible_at(queue.id, "receipt-1", updated_visible_at)
            .await
            .expect("set message visible at should succeed");

        let visible_at = sqlx::query_scalar::<_, chrono::NaiveDateTime>(
            "SELECT visible_at FROM sqs_messages WHERE queue_id = ? AND receipt_handle = ?",
        )
        .bind(queue.id)
        .bind("receipt-1")
        .fetch_one(&store.pool)
        .await
        .expect("visible_at query should succeed");

        assert_eq!(visible_at, updated_visible_at);
    }

    #[tokio::test]
    async fn set_message_visible_at_returns_not_found_for_missing_receipt_handle() {
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

        let result = store
            .set_message_visible_at(queue.id, "missing-receipt", Utc::now().naive_utc())
            .await;

        assert!(matches!(result, Err(StoreError::NotFound(_))));
    }

    #[tokio::test]
    async fn set_message_visible_at_scopes_update_to_queue_id() {
        let (_temp_dir, store) = test_store().await;

        store
            .create_queue(queue("orders", "us-east-1"))
            .await
            .expect("orders queue insert should succeed");
        store
            .create_queue(queue("payments", "us-east-1"))
            .await
            .expect("payments queue insert should succeed");

        let orders_queue = store
            .get_queue("orders", "us-east-1", "123456789012")
            .await
            .expect("orders queue lookup should succeed")
            .expect("orders queue should exist");
        let payments_queue = store
            .get_queue("payments", "us-east-1", "123456789012")
            .await
            .expect("payments queue lookup should succeed")
            .expect("payments queue should exist");

        let now = Utc::now().naive_utc();
        let orders_initial_visible_at = now - Duration::seconds(10);
        let payments_initial_visible_at = now - Duration::seconds(5);
        let updated_visible_at = now + Duration::minutes(3);

        store
            .send_message(&SqsMessage {
                receipt_handle: Some("receipt-1".to_string()),
                ..message(
                    orders_queue.id,
                    "orders-msg-1",
                    now - Duration::seconds(20),
                    orders_initial_visible_at,
                    now + Duration::hours(1),
                )
            })
            .await
            .expect("orders message should insert");
        store
            .send_message(&SqsMessage {
                receipt_handle: Some("receipt-1".to_string()),
                ..message(
                    payments_queue.id,
                    "payments-msg-1",
                    now - Duration::seconds(15),
                    payments_initial_visible_at,
                    now + Duration::hours(1),
                )
            })
            .await
            .expect("payments message should insert");

        store
            .set_message_visible_at(orders_queue.id, "receipt-1", updated_visible_at)
            .await
            .expect("set message visible at should succeed");

        let orders_visible_at = sqlx::query_scalar::<_, chrono::NaiveDateTime>(
            "SELECT visible_at FROM sqs_messages WHERE queue_id = ? AND receipt_handle = ?",
        )
        .bind(orders_queue.id)
        .bind("receipt-1")
        .fetch_one(&store.pool)
        .await
        .expect("orders visible_at query should succeed");
        let payments_visible_at = sqlx::query_scalar::<_, chrono::NaiveDateTime>(
            "SELECT visible_at FROM sqs_messages WHERE queue_id = ? AND receipt_handle = ?",
        )
        .bind(payments_queue.id)
        .bind("receipt-1")
        .fetch_one(&store.pool)
        .await
        .expect("payments visible_at query should succeed");

        assert_eq!(orders_visible_at, updated_visible_at);
        assert_eq!(payments_visible_at, payments_initial_visible_at);
    }
}
