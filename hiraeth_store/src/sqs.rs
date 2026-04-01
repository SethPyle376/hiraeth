use async_trait::async_trait;
use crate::StorageError;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SqsStoreError {
    StorageError(StorageError),
}

pub struct SqsQueue {
    pub name: String,
    pub region: String,
    pub account_id: String,
    pub queue_type: String,
    pub visibility_timeout_seconds: u32,
    pub delay_seconds: u32,
    pub message_retention_period_seconds: u32,
    pub receive_message_wait_time_seconds: u32,
}

#[async_trait]
pub trait SqsStore {
    async fn create_queue(&self, queue: SqsQueue) -> Result<(), SqsStoreError>;
}
