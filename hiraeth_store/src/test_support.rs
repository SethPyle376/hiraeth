use std::{
    collections::{HashSet, VecDeque},
    sync::{
        Mutex,
        atomic::{AtomicUsize, Ordering},
    },
};

use async_trait::async_trait;

use crate::{
    StoreError,
    sqs::{SqsMessage, SqsQueue, SqsQueueAttributeUpdate, SqsStore},
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ListQueuesCall {
    pub region: String,
    pub account_id: String,
    pub queue_name_prefix: Option<String>,
    pub max_results: Option<i64>,
    pub next_token: Option<String>,
}

#[derive(Debug)]
pub struct SqsTestStore {
    queues: Mutex<Vec<SqsQueue>>,
    created_queues: Mutex<Vec<SqsQueue>>,
    deleted_queue_ids: Mutex<Vec<i64>>,
    purged_queue_ids: Mutex<Vec<i64>>,
    sent_messages: Mutex<Vec<SqsMessage>>,
    receive_responses: Mutex<VecDeque<Vec<SqsMessage>>>,
    receive_calls: AtomicUsize,
    deleted_messages: Mutex<Vec<(i64, String)>>,
    failing_receipt_handles: HashSet<String>,
    visibility_updates: Mutex<Vec<(i64, String, chrono::NaiveDateTime)>>,
    list_queues_calls: Mutex<Vec<ListQueuesCall>>,
    message_count: Option<i64>,
    visible_message_count: Option<i64>,
    delayed_message_count: Option<i64>,
}

impl Default for SqsTestStore {
    fn default() -> Self {
        Self {
            queues: Mutex::new(Vec::new()),
            created_queues: Mutex::new(Vec::new()),
            deleted_queue_ids: Mutex::new(Vec::new()),
            purged_queue_ids: Mutex::new(Vec::new()),
            sent_messages: Mutex::new(Vec::new()),
            receive_responses: Mutex::new(VecDeque::new()),
            receive_calls: AtomicUsize::new(0),
            deleted_messages: Mutex::new(Vec::new()),
            failing_receipt_handles: HashSet::new(),
            visibility_updates: Mutex::new(Vec::new()),
            list_queues_calls: Mutex::new(Vec::new()),
            message_count: None,
            visible_message_count: None,
            delayed_message_count: None,
        }
    }
}

impl SqsTestStore {
    pub fn with_queue(queue: SqsQueue) -> Self {
        Self::with_queues([queue])
    }

    pub fn with_queues(queues: impl IntoIterator<Item = SqsQueue>) -> Self {
        Self {
            queues: Mutex::new(queues.into_iter().collect()),
            ..Self::default()
        }
    }

    pub fn with_receive_responses(
        mut self,
        receive_responses: impl IntoIterator<Item = Vec<SqsMessage>>,
    ) -> Self {
        self.receive_responses = Mutex::new(receive_responses.into_iter().collect());
        self
    }

    pub fn with_message_counts(
        mut self,
        message_count: i64,
        visible_message_count: i64,
        delayed_message_count: i64,
    ) -> Self {
        self.message_count = Some(message_count);
        self.visible_message_count = Some(visible_message_count);
        self.delayed_message_count = Some(delayed_message_count);
        self
    }

    pub fn with_failing_receipt_handles(mut self, receipt_handles: &[&str]) -> Self {
        self.failing_receipt_handles = receipt_handles
            .iter()
            .map(|receipt_handle| receipt_handle.to_string())
            .collect();
        self
    }

    pub fn created_queues(&self) -> Vec<SqsQueue> {
        self.created_queues
            .lock()
            .expect("created queues mutex")
            .clone()
    }

    pub fn deleted_queue_ids(&self) -> Vec<i64> {
        self.deleted_queue_ids
            .lock()
            .expect("deleted queue ids mutex")
            .clone()
    }

    pub fn purged_queue_ids(&self) -> Vec<i64> {
        self.purged_queue_ids
            .lock()
            .expect("purged queue ids mutex")
            .clone()
    }

    pub fn sent_messages(&self) -> Vec<SqsMessage> {
        self.sent_messages
            .lock()
            .expect("sent messages mutex")
            .clone()
    }

    pub fn receive_calls(&self) -> usize {
        self.receive_calls.load(Ordering::SeqCst)
    }

    pub fn deleted_messages(&self) -> Vec<(i64, String)> {
        self.deleted_messages
            .lock()
            .expect("deleted messages mutex")
            .clone()
    }

    pub fn visibility_updates(&self) -> Vec<(i64, String, chrono::NaiveDateTime)> {
        self.visibility_updates
            .lock()
            .expect("visibility updates mutex")
            .clone()
    }

    pub fn list_queues_calls(&self) -> Vec<ListQueuesCall> {
        self.list_queues_calls
            .lock()
            .expect("list queues calls mutex")
            .clone()
    }

    fn sent_message_count(&self, queue_id: i64) -> i64 {
        self.sent_messages
            .lock()
            .expect("sent messages mutex")
            .iter()
            .filter(|message| message.queue_id == queue_id)
            .count() as i64
    }
}

#[async_trait]
impl SqsStore for SqsTestStore {
    async fn create_queue(&self, queue: SqsQueue) -> Result<(), StoreError> {
        self.queues
            .lock()
            .expect("queues mutex")
            .push(queue.clone());
        self.created_queues
            .lock()
            .expect("created queues mutex")
            .push(queue);
        Ok(())
    }

    async fn delete_queue(&self, queue_id: i64) -> Result<(), StoreError> {
        self.deleted_queue_ids
            .lock()
            .expect("deleted queue ids mutex")
            .push(queue_id);
        self.queues
            .lock()
            .expect("queues mutex")
            .retain(|queue| queue.id != queue_id);
        Ok(())
    }

    async fn get_queue(
        &self,
        queue_name: &str,
        region: &str,
        account_id: &str,
    ) -> Result<Option<SqsQueue>, StoreError> {
        Ok(self
            .queues
            .lock()
            .expect("queues mutex")
            .iter()
            .find(|queue| {
                queue.name == queue_name && queue.region == region && queue.account_id == account_id
            })
            .cloned())
    }

    async fn get_message_count(&self, queue_id: i64) -> Result<i64, StoreError> {
        Ok(self
            .message_count
            .unwrap_or_else(|| self.sent_message_count(queue_id)))
    }

    async fn get_visible_message_count(&self, queue_id: i64) -> Result<i64, StoreError> {
        Ok(self
            .visible_message_count
            .unwrap_or_else(|| self.sent_message_count(queue_id)))
    }

    async fn get_messages_delayed_count(&self, _queue_id: i64) -> Result<i64, StoreError> {
        Ok(self.delayed_message_count.unwrap_or(0))
    }

    async fn list_queues(
        &self,
        region: &str,
        account_id: &str,
        queue_name_prefix: Option<&str>,
        max_results: Option<i64>,
        next_token: Option<&str>,
    ) -> Result<Vec<SqsQueue>, StoreError> {
        self.list_queues_calls
            .lock()
            .expect("list queues calls mutex")
            .push(ListQueuesCall {
                region: region.to_string(),
                account_id: account_id.to_string(),
                queue_name_prefix: queue_name_prefix.map(str::to_string),
                max_results,
                next_token: next_token.map(str::to_string),
            });

        let mut queues = self
            .queues
            .lock()
            .expect("queues mutex")
            .iter()
            .filter(|queue| queue.region == region && queue.account_id == account_id)
            .filter(|queue| {
                queue_name_prefix
                    .map(|prefix| queue.name.starts_with(prefix))
                    .unwrap_or(true)
            })
            .filter(|queue| {
                next_token
                    .map(|token| queue.name.as_str() > token)
                    .unwrap_or(true)
            })
            .cloned()
            .collect::<Vec<_>>();

        queues.sort_by(|left, right| left.name.cmp(&right.name));

        if let Some(max_results) = max_results {
            queues.truncate(max_results as usize);
        }

        Ok(queues)
    }

    async fn purge_queue(&self, queue_id: i64) -> Result<(), StoreError> {
        self.purged_queue_ids
            .lock()
            .expect("purged queue ids mutex")
            .push(queue_id);
        self.sent_messages
            .lock()
            .expect("sent messages mutex")
            .retain(|message| message.queue_id != queue_id);
        Ok(())
    }

    async fn set_queue_attributes(
        &self,
        queue_id: i64,
        attributes: SqsQueueAttributeUpdate,
    ) -> Result<(), StoreError> {
        if let Some(queue) = self
            .queues
            .lock()
            .expect("queues mutex")
            .iter_mut()
            .find(|queue| queue.id == queue_id)
        {
            apply_queue_attribute_update(queue, attributes);
        }

        Ok(())
    }

    async fn send_message(&self, message: &SqsMessage) -> Result<(), StoreError> {
        self.sent_messages
            .lock()
            .expect("sent messages mutex")
            .push(message.clone());
        Ok(())
    }

    async fn receive_messages(
        &self,
        queue_id: i64,
        max_number_of_messages: i64,
        _visibility_timeout_seconds: u32,
    ) -> Result<Vec<SqsMessage>, StoreError> {
        self.receive_calls.fetch_add(1, Ordering::SeqCst);

        let messages = self
            .receive_responses
            .lock()
            .expect("receive responses mutex")
            .pop_front()
            .unwrap_or_default();

        Ok(messages
            .into_iter()
            .filter(|message| message.queue_id == queue_id)
            .take(max_number_of_messages as usize)
            .collect())
    }

    async fn delete_message(&self, queue_id: i64, receipt_handle: &str) -> Result<(), StoreError> {
        if self.failing_receipt_handles.contains(receipt_handle) {
            return Err(StoreError::StorageFailure(format!(
                "failed to delete {}",
                receipt_handle
            )));
        }

        self.deleted_messages
            .lock()
            .expect("deleted messages mutex")
            .push((queue_id, receipt_handle.to_string()));

        Ok(())
    }

    async fn set_message_visible_at(
        &self,
        queue_id: i64,
        receipt_handle: &str,
        visible_at: chrono::NaiveDateTime,
    ) -> Result<(), StoreError> {
        if self.failing_receipt_handles.contains(receipt_handle) {
            return Err(StoreError::StorageFailure(format!(
                "failed to update visibility for {}",
                receipt_handle
            )));
        }

        self.visibility_updates
            .lock()
            .expect("visibility updates mutex")
            .push((queue_id, receipt_handle.to_string(), visible_at));
        Ok(())
    }
}

fn apply_queue_attribute_update(queue: &mut SqsQueue, attributes: SqsQueueAttributeUpdate) {
    if let Some(value) = attributes.visibility_timeout_seconds {
        queue.visibility_timeout_seconds = value;
    }
    if let Some(value) = attributes.delay_seconds {
        queue.delay_seconds = value;
    }
    if let Some(value) = attributes.maximum_message_size {
        queue.maximum_message_size = value;
    }
    if let Some(value) = attributes.message_retention_period_seconds {
        queue.message_retention_period_seconds = value;
    }
    if let Some(value) = attributes.receive_message_wait_time_seconds {
        queue.receive_message_wait_time_seconds = value;
    }
    if let Some(value) = attributes.policy {
        queue.policy = value;
    }
    if let Some(value) = attributes.redrive_policy {
        queue.redrive_policy = value;
    }
    if let Some(value) = attributes.content_based_deduplication {
        queue.content_based_deduplication = value;
    }
    if let Some(value) = attributes.kms_master_key_id {
        queue.kms_master_key_id = value;
    }
    if let Some(value) = attributes.kms_data_key_reuse_period_seconds {
        queue.kms_data_key_reuse_period_seconds = value;
    }
    if let Some(value) = attributes.deduplication_scope {
        queue.deduplication_scope = value;
    }
    if let Some(value) = attributes.fifo_throughput_limit {
        queue.fifo_throughput_limit = value;
    }
    if let Some(value) = attributes.redrive_allow_policy {
        queue.redrive_allow_policy = value;
    }
    if let Some(value) = attributes.sqs_managed_sse_enabled {
        queue.sqs_managed_sse_enabled = value;
    }

    queue.updated_at = chrono::Utc::now().naive_utc();
}
