use async_trait::async_trait;
use hiraeth_iam::AuthorizationMode;
use hiraeth_store::StoreError;
use hiraeth_store::sns::{SnsStore, SnsSubscription, SnsTopic};
use hiraeth_store::sqs::SqsStore;

/// Composite store passed to SNS actions. It provides the SNS store interface
/// while also giving actions access to the SQS store and authorization mode
/// for cross-service delivery.
#[derive(Clone)]
pub struct SnsServiceStore<SS, QS> {
    pub sns_store: SS,
    pub sqs_store: QS,
    pub auth_mode: AuthorizationMode,
}

impl<SS, QS> SnsServiceStore<SS, QS> {
    pub fn new(sns_store: SS, sqs_store: QS, auth_mode: AuthorizationMode) -> Self {
        Self {
            sns_store,
            sqs_store,
            auth_mode,
        }
    }
}

#[async_trait]
impl<SS, QS> SnsStore for SnsServiceStore<SS, QS>
where
    SS: SnsStore + Send + Sync,
    QS: Send + Sync,
{
    async fn create_topic(&self, topic: SnsTopic) -> Result<(), StoreError> {
        self.sns_store.create_topic(topic).await
    }

    async fn get_topic(&self, topic_arn: &str) -> Result<Option<SnsTopic>, StoreError> {
        self.sns_store.get_topic(topic_arn).await
    }

    async fn get_topic_by_id(&self, id: i64) -> Result<Option<SnsTopic>, StoreError> {
        self.sns_store.get_topic_by_id(id).await
    }

    async fn list_topics(
        &self,
        region: &str,
        account_id: &str,
        prefix: Option<&str>,
        limit: Option<i64>,
    ) -> Result<Vec<SnsTopic>, StoreError> {
        self.sns_store
            .list_topics(region, account_id, prefix, limit)
            .await
    }

    async fn delete_topic(&self, topic_arn: &str) -> Result<(), StoreError> {
        self.sns_store.delete_topic(topic_arn).await
    }

    async fn create_subscription(&self, subscription: SnsSubscription) -> Result<(), StoreError> {
        self.sns_store.create_subscription(subscription).await
    }

    async fn get_subscription(
        &self,
        subscription_arn: &str,
    ) -> Result<Option<SnsSubscription>, StoreError> {
        self.sns_store.get_subscription(subscription_arn).await
    }

    async fn get_subscription_by_id(&self, id: i64) -> Result<Option<SnsSubscription>, StoreError> {
        self.sns_store.get_subscription_by_id(id).await
    }

    async fn list_subscriptions_by_topic(
        &self,
        topic_arn: &str,
    ) -> Result<Vec<SnsSubscription>, StoreError> {
        self.sns_store.list_subscriptions_by_topic(topic_arn).await
    }

    async fn delete_subscription(&self, subscription_arn: &str) -> Result<(), StoreError> {
        self.sns_store.delete_subscription(subscription_arn).await
    }

    async fn delete_subscription_by_id(&self, id: i64) -> Result<(), StoreError> {
        self.sns_store.delete_subscription_by_id(id).await
    }
}
