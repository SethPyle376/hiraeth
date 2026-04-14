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
    pub maximum_message_size: i64,
    pub message_retention_period_seconds: i64,
    pub receive_message_wait_time_seconds: i64,
    pub policy: String,
    pub redrive_policy: String,
    pub content_based_deduplication: bool,
    pub kms_master_key_id: Option<String>,
    pub kms_data_key_reuse_period_seconds: i64,
    pub deduplication_scope: String,
    pub fifo_throughput_limit: String,
    pub redrive_allow_policy: String,
    pub sqs_managed_sse_enabled: bool,
    pub created_at: chrono::NaiveDateTime,
    pub updated_at: chrono::NaiveDateTime,
}

impl Default for SqsQueue {
    fn default() -> Self {
        let timestamp = chrono::NaiveDate::from_ymd_opt(1970, 1, 1)
            .expect("default queue date should be valid")
            .and_hms_opt(0, 0, 0)
            .expect("default queue time should be valid");

        Self {
            id: 0,
            name: String::new(),
            region: String::new(),
            account_id: String::new(),
            queue_type: "standard".to_string(),
            visibility_timeout_seconds: 30,
            delay_seconds: 0,
            maximum_message_size: 1048576,
            message_retention_period_seconds: 345600,
            receive_message_wait_time_seconds: 0,
            policy: "{}".to_string(),
            redrive_policy: "{}".to_string(),
            content_based_deduplication: false,
            kms_master_key_id: None,
            kms_data_key_reuse_period_seconds: 300,
            deduplication_scope: "queue".to_string(),
            fifo_throughput_limit: "perQueue".to_string(),
            redrive_allow_policy: "{}".to_string(),
            sqs_managed_sse_enabled: false,
            created_at: timestamp,
            updated_at: timestamp,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SqsMessage {
    pub message_id: String,
    pub queue_id: i64,
    pub body: String,
    pub message_attributes: Option<String>,
    pub aws_trace_header: Option<String>,
    pub sent_at: chrono::NaiveDateTime,
    pub visible_at: chrono::NaiveDateTime,
    pub expires_at: chrono::NaiveDateTime,
    pub receive_count: i64,
    pub receipt_handle: Option<String>,
    pub first_received_at: Option<chrono::NaiveDateTime>,
    pub message_group_id: Option<String>,
    pub message_deduplication_id: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SqsQueueAttributeUpdate {
    pub visibility_timeout_seconds: Option<i64>,
    pub delay_seconds: Option<i64>,
    pub maximum_message_size: Option<i64>,
    pub message_retention_period_seconds: Option<i64>,
    pub receive_message_wait_time_seconds: Option<i64>,
    pub policy: Option<String>,
    pub redrive_policy: Option<String>,
    pub content_based_deduplication: Option<bool>,
    pub kms_master_key_id: Option<Option<String>>,
    pub kms_data_key_reuse_period_seconds: Option<i64>,
    pub deduplication_scope: Option<String>,
    pub fifo_throughput_limit: Option<String>,
    pub redrive_allow_policy: Option<String>,
    pub sqs_managed_sse_enabled: Option<bool>,
}

#[async_trait]
pub trait SqsStore {
    // queue ops
    async fn create_queue(&self, queue: SqsQueue) -> Result<(), StoreError>;
    async fn delete_queue(&self, queue_id: i64) -> Result<(), StoreError>;
    async fn get_queue(
        &self,
        queue_name: &str,
        region: &str,
        account_id: &str,
    ) -> Result<Option<SqsQueue>, StoreError>;
    async fn get_message_count(&self, queue_id: i64) -> Result<i64, StoreError>;
    async fn get_visible_message_count(&self, queue_id: i64) -> Result<i64, StoreError>;
    async fn get_messages_delayed_count(&self, queue_id: i64) -> Result<i64, StoreError>;
    async fn list_queues(
        &self,
        region: &str,
        account_id: &str,
        queue_name_prefix: Option<&str>,
        max_results: Option<i64>,
        next_token: Option<&str>,
    ) -> Result<Vec<SqsQueue>, StoreError>;
    async fn purge_queue(&self, queue_id: i64) -> Result<(), StoreError>;
    async fn set_queue_attributes(
        &self,
        queue_id: i64,
        attributes: SqsQueueAttributeUpdate,
    ) -> Result<(), StoreError>;

    // message ops
    async fn send_message(&self, message: &SqsMessage) -> Result<(), StoreError>;
    async fn receive_messages(
        &self,
        queue_id: i64,
        max_number_of_messages: i64,
        visibility_timeout_seconds: u32,
    ) -> Result<Vec<SqsMessage>, StoreError>;
    async fn delete_message(&self, queue_id: i64, receipt_handle: &str) -> Result<(), StoreError>;
    async fn set_message_visible_at(
        &self,
        queue_id: i64,
        receipt_handle: &str,
        visible_at: chrono::NaiveDateTime,
    ) -> Result<(), StoreError>;
}
