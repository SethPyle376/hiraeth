mod action_support;
mod create_topic;
mod get_topic_attributes;
mod publish;
mod set_topic_attributes;
mod subscribe;

use hiraeth_core::AwsActionRegistry;
use hiraeth_store::sns::SnsStore;
use hiraeth_store::sqs::SqsStore;
use serde::Serialize;

use crate::store::SnsServiceStore;

use self::{
    create_topic::CreateTopicAction, get_topic_attributes::GetTopicAttributesAction,
    publish::PublishAction, set_topic_attributes::SetTopicAttributesAction,
    subscribe::SubscribeAction,
};

pub(crate) fn registry<SS, QS>() -> AwsActionRegistry<SnsServiceStore<SS, QS>>
where
    SS: SnsStore + Send + Sync + 'static,
    QS: SqsStore + Send + Sync + 'static,
{
    let mut registry = AwsActionRegistry::new();
    registry.register(CreateTopicAction);
    registry.register(SubscribeAction);
    registry.register(PublishAction);
    registry.register(GetTopicAttributesAction);
    registry.register(SetTopicAttributesAction);
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
