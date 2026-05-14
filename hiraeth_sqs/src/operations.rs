use std::collections::HashMap;

use chrono::{DateTime, Utc};
use hiraeth_store::sqs::{SqsMessage, SqsQueue, SqsStore};

use crate::{
    error::SqsError,
    util::{
        MessageAttributeValue, calculate_message_attributes_md5, extract_aws_trace_header,
        resolve_delay_seconds, serialize_message_attributes, validate_message_body,
    },
};

/// Enqueue a message into an SQS queue. This is the cross-service primitive
/// used by SNS (and potentially other services) to deliver messages without
/// going through the full HTTP request pipeline.
#[allow(clippy::too_many_arguments)]
pub async fn enqueue_message<S: SqsStore>(
    store: &S,
    queue: &SqsQueue,
    body: String,
    message_attributes: Option<HashMap<String, MessageAttributeValue>>,
    message_system_attributes: Option<HashMap<String, MessageAttributeValue>>,
    message_group_id: Option<String>,
    message_deduplication_id: Option<String>,
    date: DateTime<Utc>,
) -> Result<String, SqsError> {
    validate_message_body(&body, queue.maximum_message_size)?;
    let delay_seconds = resolve_delay_seconds(None, queue.delay_seconds)?;

    let visible_at = date.naive_utc() + chrono::Duration::seconds(delay_seconds);
    let expires_at =
        date.naive_utc() + chrono::Duration::seconds(queue.message_retention_period_seconds);

    let serialized_message_attributes = message_attributes
        .as_ref()
        .map(serialize_message_attributes)
        .transpose()?;

    let aws_trace_header = extract_aws_trace_header(message_system_attributes.as_ref())?;

    let message = SqsMessage {
        message_id: uuid::Uuid::new_v4().to_string(),
        queue_id: queue.id,
        body,
        message_attributes: serialized_message_attributes,
        aws_trace_header,
        sent_at: date.naive_utc(),
        visible_at,
        expires_at,
        receive_count: 0,
        receipt_handle: None,
        first_received_at: None,
        message_group_id,
        message_deduplication_id,
    };

    store
        .send_message(&message)
        .await
        .map_err(|e| SqsError::InternalError(e.to_string()))?;

    Ok(message.message_id.clone())
}
