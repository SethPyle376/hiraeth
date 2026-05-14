mod action_support;
mod create_topic;
mod delete_topic;
mod get_subscription_attributes;
mod get_topic_attributes;
mod list_subscriptions;
mod list_subscriptions_by_topic;
mod list_tags_for_resource;
mod list_topics;
mod publish;
mod set_subscription_attributes;
mod set_topic_attributes;
mod subscribe;
mod tag_resource;
mod unsubscribe;
mod untag_resource;

use hiraeth_core::AwsActionRegistry;
use hiraeth_store::sns::SnsStore;
use hiraeth_store::sqs::SqsStore;
use serde::Serialize;

use crate::store::SnsServiceStore;

pub(crate) use action_support::parse_payload_error;

use self::{
    create_topic::CreateTopicAction, delete_topic::DeleteTopicAction,
    get_subscription_attributes::GetSubscriptionAttributesAction,
    get_topic_attributes::GetTopicAttributesAction, list_subscriptions::ListSubscriptionsAction,
    list_subscriptions_by_topic::ListSubscriptionsByTopicAction,
    list_tags_for_resource::ListTagsForResourceAction, list_topics::ListTopicsAction,
    publish::PublishAction, set_subscription_attributes::SetSubscriptionAttributesAction,
    set_topic_attributes::SetTopicAttributesAction, subscribe::SubscribeAction,
    tag_resource::TagResourceAction, unsubscribe::UnsubscribeAction,
    untag_resource::UntagResourceAction,
};

pub(crate) fn registry<SS, QS>() -> AwsActionRegistry<SnsServiceStore<SS, QS>>
where
    SS: SnsStore + Send + Sync + 'static,
    QS: SqsStore + Send + Sync + 'static,
{
    let mut registry = AwsActionRegistry::new();
    registry.register(CreateTopicAction);
    registry.register(DeleteTopicAction);
    registry.register(SubscribeAction);
    registry.register(PublishAction);
    registry.register(GetTopicAttributesAction);
    registry.register(GetSubscriptionAttributesAction);
    registry.register(ListSubscriptionsAction);
    registry.register(ListSubscriptionsByTopicAction);
    registry.register(ListTopicsAction);
    registry.register(SetSubscriptionAttributesAction);
    registry.register(SetTopicAttributesAction);
    registry.register(ListTagsForResourceAction);
    registry.register(TagResourceAction);
    registry.register(UntagResourceAction);
    registry.register(UnsubscribeAction);
    registry
}

#[cfg(test)]
pub(crate) mod test_support {
    use hiraeth_core::ResolvedRequest;

    pub(crate) fn parse_request_body<T>(request: &ResolvedRequest) -> T
    where
        T: serde::de::DeserializeOwned,
    {
        let body = String::from_utf8_lossy(&request.request.body);
        serde_urlencoded::from_str(&body).expect("test request body should parse")
    }
}
