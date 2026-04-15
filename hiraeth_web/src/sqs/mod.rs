use std::collections::BTreeMap;

use askama::Template;
use axum::{
    Json, Router,
    extract::{Path, Query, State},
    response::{Html, Redirect},
    routing::{get, post},
};
use chrono::Utc;
use hiraeth_store::{
    StoreError,
    sqs::{SqsMessage, SqsQueue, SqsStore},
};
use serde::{Deserialize, Serialize};

use crate::{
    WebState,
    error::WebError,
    templates::{
        QueueListTemplate, SqsDashboardTemplate, SqsQueueDetailTemplate, SqsQueuesTemplate,
    },
};

fn default_region() -> String {
    "us-east-1".to_string()
}

fn default_account_id() -> String {
    "000000000000".to_string()
}

#[derive(Debug, Clone, Deserialize)]
struct QueueListParams {
    #[serde(default = "default_region")]
    region: String,
    #[serde(default = "default_account_id")]
    account_id: String,
    #[serde(default)]
    prefix: Option<String>,
}

fn default_message_limit() -> i64 {
    50
}

#[derive(Debug, Clone, Deserialize)]
struct QueueDetailParams {
    #[serde(default = "default_message_limit")]
    message_limit: i64,
}

#[derive(Debug, Clone, Serialize)]
struct QueueListResponse {
    region: String,
    account_id: String,
    queues: Vec<QueueSummary>,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct QueueSummary {
    pub(crate) id: i64,
    pub(crate) name: String,
    pub(crate) region: String,
    pub(crate) account_id: String,
    pub(crate) queue_type: String,
    pub(crate) message_count: i64,
    pub(crate) visible_message_count: i64,
    pub(crate) delayed_message_count: i64,
}

pub(crate) struct QueueAttribute {
    pub(crate) name: &'static str,
    pub(crate) value: String,
}

pub(crate) struct MessageSummary {
    pub(crate) message_id: String,
    pub(crate) body_preview: String,
    pub(crate) body: String,
    pub(crate) status: &'static str,
    pub(crate) status_badge_class: &'static str,
    pub(crate) sent_at: String,
    pub(crate) visible_at: String,
    pub(crate) expires_at: String,
    pub(crate) receive_count: i64,
    pub(crate) receipt_handle: String,
    pub(crate) has_receipt_handle: bool,
    pub(crate) first_received_at: String,
    pub(crate) message_attributes: Vec<MessageAttributeSummary>,
    pub(crate) has_message_attributes: bool,
    pub(crate) raw_message_attributes: String,
    pub(crate) has_unparsed_message_attributes: bool,
    pub(crate) aws_trace_header: String,
    pub(crate) message_group_id: String,
    pub(crate) message_deduplication_id: String,
}

pub(crate) struct MessageAttributeSummary {
    pub(crate) name: String,
    pub(crate) data_type: String,
    pub(crate) value_kind: &'static str,
    pub(crate) value: String,
}

pub fn router() -> Router<WebState> {
    Router::new()
        .route("/", get(dashboard))
        .route("/queues", get(queues_page))
        .route("/queues/{queue_id}", get(queue_detail))
        .route("/queues/{queue_id}/delete", post(delete_queue))
        .route("/queues/{queue_id}/purge", post(purge_queue))
        .route(
            "/queues/{queue_id}/messages/{message_id}/delete",
            post(delete_message),
        )
        .route("/fragments/queues", get(queues_fragment))
        .route("/api/queues", get(queues_api))
}

async fn dashboard(
    State(state): State<WebState>,
    Query(params): Query<QueueListParams>,
) -> Result<Html<String>, WebError> {
    let summaries = load_queue_summaries(&state, &params).await?;
    let prefix = params.prefix.clone().unwrap_or_default();
    let total_messages = summaries.iter().map(|queue| queue.message_count).sum();
    let visible_messages = summaries
        .iter()
        .map(|queue| queue.visible_message_count)
        .sum();
    let delayed_messages = summaries
        .iter()
        .map(|queue| queue.delayed_message_count)
        .sum();

    let template = SqsDashboardTemplate {
        region: &params.region,
        account_id: &params.account_id,
        prefix: &prefix,
        total_queues: summaries.len(),
        total_messages,
        visible_messages,
        delayed_messages,
        queues: &summaries,
        has_queues: !summaries.is_empty(),
    };

    Ok(Html(template.render()?))
}

async fn queues_page(
    State(state): State<WebState>,
    Query(params): Query<QueueListParams>,
) -> Result<Html<String>, WebError> {
    let summaries = load_queue_summaries(&state, &params).await?;
    let prefix = params.prefix.clone().unwrap_or_default();
    let template = SqsQueuesTemplate {
        region: &params.region,
        account_id: &params.account_id,
        prefix: &prefix,
        queues: &summaries,
        has_queues: !summaries.is_empty(),
    };

    Ok(Html(template.render()?))
}

async fn queues_fragment(
    State(state): State<WebState>,
    Query(params): Query<QueueListParams>,
) -> Result<Html<String>, WebError> {
    let summaries = load_queue_summaries(&state, &params).await?;
    let template = QueueListTemplate {
        queues: &summaries,
        has_queues: !summaries.is_empty(),
    };

    Ok(Html(template.render()?))
}

async fn queues_api(
    State(state): State<WebState>,
    Query(params): Query<QueueListParams>,
) -> Result<Json<QueueListResponse>, WebError> {
    let summaries = load_queue_summaries(&state, &params).await?;
    Ok(Json(QueueListResponse {
        region: params.region,
        account_id: params.account_id,
        queues: summaries,
    }))
}

async fn queue_detail(
    State(state): State<WebState>,
    Path(queue_id): Path<i64>,
    Query(params): Query<QueueDetailParams>,
) -> Result<Html<String>, WebError> {
    let queue = state
        .sqs_store
        .get_queue_by_id(queue_id)
        .await?
        .ok_or_else(|| StoreError::NotFound("queue not found".to_string()))?;
    let summary = queue_summary(&state, queue.clone()).await?;
    let message_limit = params.message_limit.clamp(1, 500);
    let messages = state
        .sqs_store
        .list_messages(queue.id, message_limit)
        .await?;
    let message_summaries = messages
        .into_iter()
        .map(message_summary)
        .collect::<Vec<_>>();
    let attributes = queue_attributes(&queue);

    let template = SqsQueueDetailTemplate {
        queue: &queue,
        summary: &summary,
        attributes: &attributes,
        messages: &message_summaries,
        has_messages: !message_summaries.is_empty(),
        message_limit,
    };

    Ok(Html(template.render()?))
}

async fn purge_queue(
    State(state): State<WebState>,
    Path(queue_id): Path<i64>,
) -> Result<Redirect, WebError> {
    state.sqs_store.purge_queue(queue_id).await?;
    Ok(Redirect::to(&format!("/sqs/queues/{queue_id}")))
}

async fn delete_queue(
    State(state): State<WebState>,
    Path(queue_id): Path<i64>,
) -> Result<Redirect, WebError> {
    state.sqs_store.delete_queue(queue_id).await?;
    Ok(Redirect::to("/sqs/queues"))
}

async fn delete_message(
    State(state): State<WebState>,
    Path((queue_id, message_id)): Path<(i64, String)>,
) -> Result<Redirect, WebError> {
    state
        .sqs_store
        .delete_message_by_id(queue_id, &message_id)
        .await?;
    Ok(Redirect::to(&format!("/sqs/queues/{queue_id}")))
}

async fn load_queue_summaries(
    state: &WebState,
    params: &QueueListParams,
) -> Result<Vec<QueueSummary>, WebError> {
    let queues = state
        .sqs_store
        .list_queues(
            &params.region,
            &params.account_id,
            params.prefix.as_deref().filter(|prefix| !prefix.is_empty()),
            Some(100),
            None,
        )
        .await?;

    let mut summaries = Vec::with_capacity(queues.len());
    for queue in queues {
        summaries.push(queue_summary(state, queue).await?);
    }

    Ok(summaries)
}

async fn queue_summary(state: &WebState, queue: SqsQueue) -> Result<QueueSummary, WebError> {
    let message_count = state.sqs_store.get_message_count(queue.id).await?;
    let visible_message_count = state.sqs_store.get_visible_message_count(queue.id).await?;
    let delayed_message_count = state.sqs_store.get_messages_delayed_count(queue.id).await?;

    Ok(QueueSummary {
        id: queue.id,
        name: queue.name,
        region: queue.region,
        account_id: queue.account_id,
        queue_type: queue.queue_type,
        message_count,
        visible_message_count,
        delayed_message_count,
    })
}

fn queue_attributes(queue: &SqsQueue) -> Vec<QueueAttribute> {
    vec![
        QueueAttribute {
            name: "VisibilityTimeout",
            value: queue.visibility_timeout_seconds.to_string(),
        },
        QueueAttribute {
            name: "DelaySeconds",
            value: queue.delay_seconds.to_string(),
        },
        QueueAttribute {
            name: "MaximumMessageSize",
            value: queue.maximum_message_size.to_string(),
        },
        QueueAttribute {
            name: "MessageRetentionPeriod",
            value: queue.message_retention_period_seconds.to_string(),
        },
        QueueAttribute {
            name: "ReceiveMessageWaitTimeSeconds",
            value: queue.receive_message_wait_time_seconds.to_string(),
        },
        QueueAttribute {
            name: "ContentBasedDeduplication",
            value: queue.content_based_deduplication.to_string(),
        },
        QueueAttribute {
            name: "KmsMasterKeyId",
            value: queue.kms_master_key_id.clone().unwrap_or_default(),
        },
        QueueAttribute {
            name: "KmsDataKeyReusePeriodSeconds",
            value: queue.kms_data_key_reuse_period_seconds.to_string(),
        },
        QueueAttribute {
            name: "DeduplicationScope",
            value: queue.deduplication_scope.clone(),
        },
        QueueAttribute {
            name: "FifoThroughputLimit",
            value: queue.fifo_throughput_limit.clone(),
        },
        QueueAttribute {
            name: "SqsManagedSseEnabled",
            value: queue.sqs_managed_sse_enabled.to_string(),
        },
        QueueAttribute {
            name: "Policy",
            value: queue.policy.clone(),
        },
        QueueAttribute {
            name: "RedrivePolicy",
            value: queue.redrive_policy.clone(),
        },
        QueueAttribute {
            name: "RedriveAllowPolicy",
            value: queue.redrive_allow_policy.clone(),
        },
    ]
}

fn message_summary(message: SqsMessage) -> MessageSummary {
    let now = Utc::now().naive_utc();
    let (status, status_badge_class) = if message.expires_at <= now {
        ("Expired", "badge-error")
    } else if message.visible_at <= now {
        ("Visible", "badge-success")
    } else if message.receipt_handle.is_some() {
        ("In flight", "badge-warning")
    } else {
        ("Delayed", "badge-info")
    };

    let body_preview = preview(&message.body, 96);
    let receipt_handle = message.receipt_handle.unwrap_or_default();
    let parsed_message_attributes = parse_message_attributes(message.message_attributes);

    MessageSummary {
        message_id: message.message_id,
        body_preview,
        body: message.body,
        status,
        status_badge_class,
        sent_at: format_datetime(message.sent_at),
        visible_at: format_datetime(message.visible_at),
        expires_at: format_datetime(message.expires_at),
        receive_count: message.receive_count,
        has_receipt_handle: !receipt_handle.is_empty(),
        receipt_handle,
        first_received_at: message
            .first_received_at
            .map(format_datetime)
            .unwrap_or_default(),
        has_message_attributes: !parsed_message_attributes.attributes.is_empty(),
        message_attributes: parsed_message_attributes.attributes,
        raw_message_attributes: parsed_message_attributes.raw,
        has_unparsed_message_attributes: parsed_message_attributes.has_unparsed,
        aws_trace_header: message.aws_trace_header.unwrap_or_default(),
        message_group_id: message.message_group_id.unwrap_or_default(),
        message_deduplication_id: message.message_deduplication_id.unwrap_or_default(),
    }
}

struct ParsedMessageAttributes {
    attributes: Vec<MessageAttributeSummary>,
    raw: String,
    has_unparsed: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct StoredMessageAttribute {
    data_type: String,
    string_value: Option<String>,
    binary_value: Option<String>,
}

fn parse_message_attributes(raw: Option<String>) -> ParsedMessageAttributes {
    let raw = raw.unwrap_or_default();
    if raw.trim().is_empty() || raw.trim() == "{}" {
        return ParsedMessageAttributes {
            attributes: Vec::new(),
            raw,
            has_unparsed: false,
        };
    }

    let parsed = serde_json::from_str::<BTreeMap<String, StoredMessageAttribute>>(&raw);
    let Ok(parsed) = parsed else {
        return ParsedMessageAttributes {
            attributes: Vec::new(),
            raw,
            has_unparsed: true,
        };
    };

    let attributes = parsed
        .into_iter()
        .map(|(name, attribute)| {
            let is_binary = attribute.data_type.starts_with("Binary");
            let value = if is_binary {
                attribute.binary_value.unwrap_or_default()
            } else {
                attribute.string_value.unwrap_or_default()
            };

            MessageAttributeSummary {
                name,
                data_type: attribute.data_type,
                value_kind: if is_binary {
                    "BinaryValue"
                } else {
                    "StringValue"
                },
                value,
            }
        })
        .collect::<Vec<_>>();

    ParsedMessageAttributes {
        attributes,
        raw,
        has_unparsed: false,
    }
}

fn preview(value: &str, max_chars: usize) -> String {
    let mut preview = value.chars().take(max_chars).collect::<String>();
    if value.chars().count() > max_chars {
        preview.push_str("...");
    }
    preview
}

fn format_datetime(value: chrono::NaiveDateTime) -> String {
    value.format("%Y-%m-%d %H:%M:%S UTC").to_string()
}

#[cfg(test)]
mod tests {
    use super::parse_message_attributes;

    #[test]
    fn parse_message_attributes_extracts_attribute_names_and_values() {
        let parsed = parse_message_attributes(Some(
            r#"{"key":{"DataType":"String","StringValue":"value","BinaryValue":null}}"#.to_string(),
        ));

        assert!(!parsed.has_unparsed);
        assert_eq!(parsed.attributes.len(), 1);
        assert_eq!(parsed.attributes[0].name, "key");
        assert_eq!(parsed.attributes[0].data_type, "String");
        assert_eq!(parsed.attributes[0].value_kind, "StringValue");
        assert_eq!(parsed.attributes[0].value, "value");
    }

    #[test]
    fn parse_message_attributes_falls_back_to_raw_for_invalid_json() {
        let parsed = parse_message_attributes(Some("not-json".to_string()));

        assert!(parsed.has_unparsed);
        assert!(parsed.attributes.is_empty());
        assert_eq!(parsed.raw, "not-json");
    }
}
