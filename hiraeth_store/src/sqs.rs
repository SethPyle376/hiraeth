use async_trait::async_trait;

use crate::StoreError;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SqsQueue {
    pub name: String,
    pub region: String,
    pub account_id: String,
    pub queue_type: String,
    pub visibility_timeout_seconds: i64,
    pub delay_seconds: i64,
    pub message_retention_period_seconds: i64,
    pub receive_message_wait_time_seconds: i64,
}

#[async_trait]
pub trait SqsStore {
    async fn create_queue(&self, queue: SqsQueue) -> Result<(), StoreError>;
    async fn get_queue(
        &self,
        queue_name: &str,
        region: &str,
    ) -> Result<Option<SqsQueue>, StoreError>;
}
