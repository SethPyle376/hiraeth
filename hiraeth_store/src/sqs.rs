use async_trait::async_trait;

use crate::StoreError;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SqsQueue {
    pub id: i64,
    pub name: String,
    pub region: String,
    pub account_id: String,
    pub queue_type: String,
    pub visibility_timeout_seconds: i64,
    pub delay_seconds: i64,
    pub message_retention_period_seconds: i64,
    pub receive_message_wait_time_seconds: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SqsMessage {
    pub message_id: String,
    pub queue_id: i64,
    pub body: String,
    pub message_attributes: Option<String>,
    pub sent_at: chrono::NaiveDateTime,
    pub visible_at: chrono::NaiveDateTime,
    pub expires_at: chrono::NaiveDateTime,
    pub receive_count: i64,
    pub receipt_handle: Option<String>,
    pub first_received_at: Option<chrono::NaiveDateTime>,
    pub message_group_id: Option<String>,
    pub message_deduplication_id: Option<String>,
}

#[async_trait]
pub trait SqsStore {
    // queue ops
    async fn create_queue(&self, queue: SqsQueue) -> Result<(), StoreError>;
    async fn get_queue(
        &self,
        queue_name: &str,
        region: &str,
        account_id: &str,
    ) -> Result<Option<SqsQueue>, StoreError>;

    // message ops
    async fn send_message(&self, message: &SqsMessage) -> Result<(), StoreError>;
}
