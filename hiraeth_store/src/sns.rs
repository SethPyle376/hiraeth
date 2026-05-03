use async_trait::async_trait;

use crate::StoreError;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SnsTopic {
    pub id: i64,
    pub name: String,
    pub region: String,
    pub account_id: String,
    pub display_name: String,
    pub policy: String,
    pub created_at: chrono::NaiveDateTime,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SnsSubscription {
    pub id: i64,
    pub topic_arn: String,
    pub protocol: String,
    pub endpoint: String,
    pub owner_account_id: String,
    pub subscription_arn: String,
    pub created_at: chrono::NaiveDateTime,
}

#[async_trait]
pub trait SnsStore {
    async fn create_topic(&self, topic: SnsTopic) -> Result<(), StoreError>;
    async fn get_topic(&self, topic_arn: &str) -> Result<Option<SnsTopic>, StoreError>;
    async fn delete_topic(&self, topic_arn: &str) -> Result<(), StoreError>;

    async fn create_subscription(&self, subscription: SnsSubscription) -> Result<(), StoreError>;
    async fn get_subscription(
        &self,
        subscription_arn: &str,
    ) -> Result<Option<SnsSubscription>, StoreError>;
    async fn list_subscriptions_by_topic(
        &self,
        topic_arn: &str,
    ) -> Result<Vec<SnsSubscription>, StoreError>;
    async fn delete_subscription(&self, subscription_arn: &str) -> Result<(), StoreError>;
}
