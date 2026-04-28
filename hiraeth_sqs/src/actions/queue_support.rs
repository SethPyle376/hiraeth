use hiraeth_store::sqs::SqsQueue;

use crate::error::SqsError;

pub(crate) fn validate_queue_name(queue_name: &str, fifo_queue: bool) -> Result<(), SqsError> {
    if queue_name.is_empty() || queue_name.len() > 80 {
        return Err(SqsError::BadRequest(
            "QueueName must be between 1 and 80 characters".to_string(),
        ));
    }

    let name_without_fifo_suffix = queue_name.strip_suffix(".fifo").unwrap_or(queue_name);
    let valid_chars = name_without_fifo_suffix
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || ch == '-' || ch == '_');
    if !valid_chars {
        return Err(SqsError::BadRequest(
            "QueueName may only contain alphanumeric characters, hyphens, and underscores"
                .to_string(),
        ));
    }

    if fifo_queue && !queue_name.ends_with(".fifo") {
        return Err(SqsError::BadRequest(
            "FIFO queue names must end with .fifo".to_string(),
        ));
    }

    if !fifo_queue && queue_name.ends_with(".fifo") {
        return Err(SqsError::BadRequest(
            "Queue names ending with .fifo must set FifoQueue=true".to_string(),
        ));
    }

    Ok(())
}

pub(crate) fn queue_configuration_matches(existing: &SqsQueue, requested: &SqsQueue) -> bool {
    existing.queue_type == requested.queue_type
        && existing.visibility_timeout_seconds == requested.visibility_timeout_seconds
        && existing.delay_seconds == requested.delay_seconds
        && existing.maximum_message_size == requested.maximum_message_size
        && existing.message_retention_period_seconds == requested.message_retention_period_seconds
        && existing.receive_message_wait_time_seconds == requested.receive_message_wait_time_seconds
        && existing.policy == requested.policy
        && existing.redrive_policy == requested.redrive_policy
        && existing.content_based_deduplication == requested.content_based_deduplication
        && existing.kms_master_key_id == requested.kms_master_key_id
        && existing.kms_data_key_reuse_period_seconds == requested.kms_data_key_reuse_period_seconds
        && existing.deduplication_scope == requested.deduplication_scope
        && existing.fifo_throughput_limit == requested.fifo_throughput_limit
        && existing.redrive_allow_policy == requested.redrive_allow_policy
        && existing.sqs_managed_sse_enabled == requested.sqs_managed_sse_enabled
}
