mod action_support;
mod change_message_visibility;
mod change_message_visibility_batch;
mod create_queue;
mod delete_message;
mod delete_message_batch;
mod delete_queue;
mod get_queue_attributes;
mod get_queue_url;
mod list_queue_tags;
mod list_queues;
mod purge_queue;
mod queue_attribute_support;
mod queue_support;
mod receive_message;
mod send_message;
mod send_message_batch;
mod set_queue_attributes;
mod tag_queue;
mod tag_support;
mod untag_queue;

use hiraeth_core::AwsActionRegistry;
use hiraeth_store::sqs::SqsStore;

use self::{
    change_message_visibility::ChangeMessageVisibilityAction,
    change_message_visibility_batch::ChangeMessageVisibilityBatchAction,
    create_queue::CreateQueueAction, delete_message::DeleteMessageAction,
    delete_message_batch::DeleteMessageBatchAction, delete_queue::DeleteQueueAction,
    get_queue_attributes::GetQueueAttributesAction, get_queue_url::GetQueueUrlAction,
    list_queue_tags::ListQueueTagsAction, list_queues::ListQueuesAction,
    purge_queue::PurgeQueueAction, receive_message::ReceiveMessageAction,
    send_message::SendMessageAction, send_message_batch::SendMessageBatchAction,
    set_queue_attributes::SetQueueAttributesAction, tag_queue::TagQueueAction,
    untag_queue::UntagQueueAction,
};

pub(crate) use self::get_queue_url::GetQueueUrlRequest;

pub(crate) fn registry<S>() -> AwsActionRegistry<S>
where
    S: SqsStore + Send + Sync + 'static,
{
    let mut registry = AwsActionRegistry::new();
    registry.register_typed(ChangeMessageVisibilityAction);
    registry.register_typed(ChangeMessageVisibilityBatchAction);
    registry.register_typed(CreateQueueAction);
    registry.register_typed(DeleteMessageAction);
    registry.register_typed(DeleteMessageBatchAction);
    registry.register_typed(DeleteQueueAction);
    registry.register_typed(GetQueueAttributesAction);
    registry.register_typed(GetQueueUrlAction);
    registry.register_typed(ListQueueTagsAction);
    registry.register_typed(ListQueuesAction);
    registry.register_typed(PurgeQueueAction);
    registry.register_typed(ReceiveMessageAction);
    registry.register_typed(SendMessageAction);
    registry.register_typed(SendMessageBatchAction);
    registry.register_typed(SetQueueAttributesAction);
    registry.register_typed(TagQueueAction);
    registry.register_typed(UntagQueueAction);
    registry
}

#[cfg(test)]
mod tests {
    use hiraeth_core::AwsActionRegistry;
    use hiraeth_store::test_support::SqsTestStore;

    use super::registry;

    #[test]
    fn registers_expected_actions() {
        let registry: AwsActionRegistry<SqsTestStore> = registry();

        assert!(registry.get("CreateQueue").is_some());
        assert!(registry.get("SendMessage").is_some());
        assert!(registry.get("ReceiveMessage").is_some());
        assert!(registry.get("MissingAction").is_none());
    }
}

#[cfg(test)]
pub(crate) mod test_support {
    use hiraeth_core::ResolvedRequest;
    use serde::de::DeserializeOwned;

    pub(crate) fn parse_request_body<T>(request: &ResolvedRequest) -> T
    where
        T: DeserializeOwned,
    {
        crate::util::parse_request_body(request).expect("test request body should parse")
    }
}
