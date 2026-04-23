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
    registry.register(Box::new(ChangeMessageVisibilityAction));
    registry.register(Box::new(ChangeMessageVisibilityBatchAction));
    registry.register(Box::new(CreateQueueAction));
    registry.register(Box::new(DeleteMessageAction));
    registry.register(Box::new(DeleteMessageBatchAction));
    registry.register(Box::new(DeleteQueueAction));
    registry.register(Box::new(GetQueueAttributesAction));
    registry.register(Box::new(GetQueueUrlAction));
    registry.register(Box::new(ListQueueTagsAction));
    registry.register(Box::new(ListQueuesAction));
    registry.register(Box::new(PurgeQueueAction));
    registry.register(Box::new(ReceiveMessageAction));
    registry.register(Box::new(SendMessageAction));
    registry.register(Box::new(SendMessageBatchAction));
    registry.register(Box::new(SetQueueAttributesAction));
    registry.register(Box::new(TagQueueAction));
    registry.register(Box::new(UntagQueueAction));
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
