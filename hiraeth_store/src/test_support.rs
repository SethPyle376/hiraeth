use std::{
    collections::{HashMap, HashSet, VecDeque},
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
    queue_tags: Mutex<HashMap<i64, HashMap<String, String>>>,
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
            queue_tags: Mutex::new(HashMap::new()),
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

    pub fn queue_tags(&self, queue_id: i64) -> HashMap<String, String> {
        self.queue_tags
            .lock()
            .expect("queue tags mutex")
            .get(&queue_id)
            .cloned()
            .unwrap_or_default()
    }

    fn has_queue(&self, queue_id: i64) -> bool {
        self.queues
            .lock()
            .expect("queues mutex")
            .iter()
            .any(|queue| queue.id == queue_id)
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
        if self
            .queues
            .lock()
            .expect("queues mutex")
            .iter()
            .any(|existing| {
                existing.name == queue.name
                    && existing.region == queue.region
                    && existing.account_id == queue.account_id
            })
        {
            return Err(StoreError::Conflict(format!(
                "queue already exists: {}",
                queue.name
            )));
        }

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

    async fn list_queue_tags(&self, queue_id: i64) -> Result<HashMap<String, String>, StoreError> {
        if !self.has_queue(queue_id) {
            return Err(StoreError::NotFound("queue not found".to_string()));
        }

        Ok(self.queue_tags(queue_id))
    }

    async fn tag_queue(
        &self,
        queue_id: i64,
        tags: HashMap<String, String>,
    ) -> Result<(), StoreError> {
        if !self.has_queue(queue_id) {
            return Err(StoreError::NotFound("queue not found".to_string()));
        }

        let mut queue_tags = self.queue_tags.lock().expect("queue tags mutex");
        let tags_for_queue = queue_tags.entry(queue_id).or_default();
        for (key, value) in tags {
            tags_for_queue.insert(key, value);
        }
        Ok(())
    }

    async fn untag_queue(&self, queue_id: i64, tag_keys: Vec<String>) -> Result<(), StoreError> {
        if !self.has_queue(queue_id) {
            return Err(StoreError::NotFound("queue not found".to_string()));
        }

        let mut queue_tags = self.queue_tags.lock().expect("queue tags mutex");
        if let Some(tags_for_queue) = queue_tags.get_mut(&queue_id) {
            for key in tag_keys {
                tags_for_queue.remove(&key);
            }
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
            return Err(StoreError::NotFound(format!(
                "receipt handle not found: {}",
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
            return Err(StoreError::NotFound(format!(
                "receipt handle not found: {}",
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

// ─── SNS Test Store ───────────────────────────────────────────────────────────

use crate::sns::{SnsStore, SnsSubscription, SnsTopic};

#[derive(Debug)]
pub struct SnsTestStore {
    topics: Mutex<Vec<SnsTopic>>,
    created_topics: Mutex<Vec<SnsTopic>>,
    deleted_topic_arns: Mutex<Vec<String>>,
    subscriptions: Mutex<Vec<SnsSubscription>>,
    created_subscriptions: Mutex<Vec<SnsSubscription>>,
    deleted_subscription_arns: Mutex<Vec<String>>,
    deleted_subscription_ids: Mutex<Vec<i64>>,
}

impl Default for SnsTestStore {
    fn default() -> Self {
        Self {
            topics: Mutex::new(Vec::new()),
            created_topics: Mutex::new(Vec::new()),
            deleted_topic_arns: Mutex::new(Vec::new()),
            subscriptions: Mutex::new(Vec::new()),
            created_subscriptions: Mutex::new(Vec::new()),
            deleted_subscription_arns: Mutex::new(Vec::new()),
            deleted_subscription_ids: Mutex::new(Vec::new()),
        }
    }
}

impl SnsTestStore {
    pub fn with_topic(topic: SnsTopic) -> Self {
        Self::with_topics([topic])
    }

    pub fn with_topics(topics: impl IntoIterator<Item = SnsTopic>) -> Self {
        let topics: Vec<_> = topics.into_iter().collect();
        Self {
            topics: Mutex::new(topics.clone()),
            created_topics: Mutex::new(topics),
            ..Self::default()
        }
    }

    pub fn with_subscription(subscription: SnsSubscription) -> Self {
        Self::with_subscriptions([subscription])
    }

    pub fn with_subscriptions(subscriptions: impl IntoIterator<Item = SnsSubscription>) -> Self {
        let subscriptions: Vec<_> = subscriptions.into_iter().collect();
        Self {
            subscriptions: Mutex::new(subscriptions.clone()),
            created_subscriptions: Mutex::new(subscriptions),
            ..Self::default()
        }
    }

    pub fn created_topics(&self) -> Vec<SnsTopic> {
        self.created_topics
            .lock()
            .expect("created topics mutex")
            .clone()
    }

    pub fn deleted_topic_arns(&self) -> Vec<String> {
        self.deleted_topic_arns
            .lock()
            .expect("deleted topic arns mutex")
            .clone()
    }

    pub fn created_subscriptions(&self) -> Vec<SnsSubscription> {
        self.created_subscriptions
            .lock()
            .expect("created subscriptions mutex")
            .clone()
    }

    pub fn deleted_subscription_arns(&self) -> Vec<String> {
        self.deleted_subscription_arns
            .lock()
            .expect("deleted subscription arns mutex")
            .clone()
    }

    pub fn deleted_subscription_ids(&self) -> Vec<i64> {
        self.deleted_subscription_ids
            .lock()
            .expect("deleted subscription ids mutex")
            .clone()
    }

    fn parse_topic_arn(arn: &str) -> Option<(String, String, String)> {
        let parts: Vec<&str> = arn.split(':').collect();
        if parts.len() == 6 {
            Some((parts[3].to_string(), parts[4].to_string(), parts[5].to_string()))
        } else {
            None
        }
    }

    fn next_topic_id(&self) -> i64 {
        self.topics
            .lock()
            .expect("topics mutex")
            .iter()
            .map(|t| t.id)
            .max()
            .unwrap_or(0)
            + 1
    }

    fn next_subscription_id(&self) -> i64 {
        self.subscriptions
            .lock()
            .expect("subscriptions mutex")
            .iter()
            .map(|s| s.id)
            .max()
            .unwrap_or(0)
            + 1
    }
}

#[async_trait]
impl SnsStore for SnsTestStore {
    async fn create_topic(&self, mut topic: SnsTopic) -> Result<(), StoreError> {
        if self
            .topics
            .lock()
            .expect("topics mutex")
            .iter()
            .any(|existing| {
                existing.name == topic.name
                    && existing.region == topic.region
                    && existing.account_id == topic.account_id
            })
        {
            return Err(StoreError::Conflict(format!(
                "topic already exists: {}",
                topic.name
            )));
        }

        topic.id = self.next_topic_id();
        self.topics
            .lock()
            .expect("topics mutex")
            .push(topic.clone());
        self.created_topics
            .lock()
            .expect("created topics mutex")
            .push(topic);
        Ok(())
    }

    async fn get_topic(&self, topic_arn: &str) -> Result<Option<SnsTopic>, StoreError> {
        let (region, account_id, name) =
            Self::parse_topic_arn(topic_arn).ok_or_else(|| {
                StoreError::NotFound("topic not found".to_string())
            })?;
        Ok(self
            .topics
            .lock()
            .expect("topics mutex")
            .iter()
            .find(|topic| {
                topic.name == name && topic.region == region && topic.account_id == account_id
            })
            .cloned())
    }

    async fn get_topic_by_id(&self, id: i64) -> Result<Option<SnsTopic>, StoreError> {
        Ok(self
            .topics
            .lock()
            .expect("topics mutex")
            .iter()
            .find(|topic| topic.id == id)
            .cloned())
    }

    async fn list_topics(
        &self,
        region: &str,
        account_id: &str,
        prefix: Option<&str>,
        limit: Option<i64>,
    ) -> Result<Vec<SnsTopic>, StoreError> {
        let mut topics: Vec<_> = self
            .topics
            .lock()
            .expect("topics mutex")
            .iter()
            .filter(|topic| topic.region == region && topic.account_id == account_id)
            .filter(|topic| {
                prefix
                    .map(|p| topic.name.starts_with(p))
                    .unwrap_or(true)
            })
            .cloned()
            .collect();
        topics.sort_by(|a, b| a.name.cmp(&b.name));
        if let Some(limit) = limit {
            topics.truncate(limit as usize);
        }
        Ok(topics)
    }

    async fn delete_topic(&self, topic_arn: &str) -> Result<(), StoreError> {
        let (region, account_id, name) =
            Self::parse_topic_arn(topic_arn).ok_or_else(|| {
                StoreError::NotFound("topic not found".to_string())
            })?;
        let mut topics = self.topics.lock().expect("topics mutex");
        let before = topics.len();
        topics.retain(|topic| {
            !(topic.name == name && topic.region == region && topic.account_id == account_id)
        });
        if topics.len() == before {
            return Err(StoreError::NotFound("topic not found".to_string()));
        }
        drop(topics);

        self.deleted_topic_arns
            .lock()
            .expect("deleted topic arns mutex")
            .push(topic_arn.to_string());

        self.subscriptions
            .lock()
            .expect("subscriptions mutex")
            .retain(|sub| sub.topic_arn != topic_arn);

        Ok(())
    }

    async fn create_subscription(
        &self,
        mut subscription: SnsSubscription,
    ) -> Result<(), StoreError> {
        subscription.id = self.next_subscription_id();
        self.subscriptions
            .lock()
            .expect("subscriptions mutex")
            .push(subscription.clone());
        self.created_subscriptions
            .lock()
            .expect("created subscriptions mutex")
            .push(subscription);
        Ok(())
    }

    async fn get_subscription(
        &self,
        subscription_arn: &str,
    ) -> Result<Option<SnsSubscription>, StoreError> {
        Ok(self
            .subscriptions
            .lock()
            .expect("subscriptions mutex")
            .iter()
            .find(|sub| sub.subscription_arn == subscription_arn)
            .cloned())
    }

    async fn get_subscription_by_id(
        &self,
        id: i64,
    ) -> Result<Option<SnsSubscription>, StoreError> {
        Ok(self
            .subscriptions
            .lock()
            .expect("subscriptions mutex")
            .iter()
            .find(|sub| sub.id == id)
            .cloned())
    }

    async fn list_subscriptions_by_topic(
        &self,
        topic_arn: &str,
    ) -> Result<Vec<SnsSubscription>, StoreError> {
        Ok(self
            .subscriptions
            .lock()
            .expect("subscriptions mutex")
            .iter()
            .filter(|sub| sub.topic_arn == topic_arn)
            .cloned()
            .collect())
    }

    async fn delete_subscription(&self, subscription_arn: &str) -> Result<(), StoreError> {
        let mut subs = self.subscriptions.lock().expect("subscriptions mutex");
        let before = subs.len();
        subs.retain(|sub| sub.subscription_arn != subscription_arn);
        if subs.len() == before {
            return Err(StoreError::NotFound("subscription not found".to_string()));
        }
        drop(subs);
        self.deleted_subscription_arns
            .lock()
            .expect("deleted subscription arns mutex")
            .push(subscription_arn.to_string());
        Ok(())
    }

    async fn delete_subscription_by_id(&self, id: i64) -> Result<(), StoreError> {
        let mut subs = self.subscriptions.lock().expect("subscriptions mutex");
        let before = subs.len();
        subs.retain(|sub| sub.id != id);
        if subs.len() == before {
            return Err(StoreError::NotFound("subscription not found".to_string()));
        }
        drop(subs);
        self.deleted_subscription_ids
            .lock()
            .expect("deleted subscription ids mutex")
            .push(id);
        Ok(())
    }
}
