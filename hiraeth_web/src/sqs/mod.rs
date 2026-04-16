use std::collections::BTreeMap;

use askama::Template;
use axum::{
    Json, Router,
    extract::{Form, Path, Query, State},
    response::{Html, Redirect},
    routing::{get, post},
};
use chrono::{Duration, Utc};
use hiraeth_store::{
    StoreError,
    sqs::{SqsMessage, SqsQueue, SqsStore},
};
use serde::{Deserialize, Serialize};

use crate::{
    WebState,
    error::WebError,
    templates::{
        QueueDetailStatsTemplate, QueueListTemplate, QueueMessageListTemplate,
        SqsDashboardStatsTemplate, SqsDashboardTemplate, SqsQueueDetailTemplate, SqsQueuesTemplate,
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

#[derive(Debug, Clone, Deserialize)]
struct CreateQueueForm {
    queue_name: String,
    region: String,
    account_id: String,
    queue_type: String,
}

#[derive(Debug, Clone, Deserialize)]
struct SendMessageForm {
    message_body: String,
    #[serde(default)]
    delay_seconds: String,
    #[serde(default)]
    message_attributes_json: String,
    #[serde(default)]
    aws_trace_header: String,
    #[serde(default)]
    message_group_id: String,
    #[serde(default)]
    message_deduplication_id: String,
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

pub(crate) struct QueueTag {
    pub(crate) key: String,
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
        .route("/queues/create", post(create_queue))
        .route("/queues/{queue_id}", get(queue_detail))
        .route("/queues/{queue_id}/delete", post(delete_queue))
        .route("/queues/{queue_id}/purge", post(purge_queue))
        .route("/queues/{queue_id}/messages", post(send_message))
        .route(
            "/queues/{queue_id}/messages/{message_id}/delete",
            post(delete_message),
        )
        .route("/fragments/queues", get(queues_fragment))
        .route("/fragments/dashboard-stats", get(dashboard_stats_fragment))
        .route(
            "/fragments/queues/{queue_id}/stats",
            get(queue_detail_stats_fragment),
        )
        .route(
            "/fragments/queues/{queue_id}/messages",
            get(queue_messages_fragment),
        )
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

async fn create_queue(
    State(state): State<WebState>,
    Form(form): Form<CreateQueueForm>,
) -> Result<Redirect, WebError> {
    let queue_name = form.queue_name.trim();
    let region = form.region.trim();
    let account_id = form.account_id.trim();
    let queue_type = form.queue_type.trim();

    validate_required("Queue name", queue_name)?;
    validate_required("Region", region)?;
    validate_required("Account ID", account_id)?;
    validate_queue_name(queue_name, queue_type)?;

    let now = Utc::now().naive_utc();
    let queue = SqsQueue {
        id: 0,
        name: queue_name.to_string(),
        region: region.to_string(),
        account_id: account_id.to_string(),
        queue_type: queue_type.to_string(),
        created_at: now,
        updated_at: now,
        ..Default::default()
    };

    state.sqs_store.create_queue(queue).await?;

    let created_queue = state
        .sqs_store
        .get_queue(queue_name, region, account_id)
        .await?
        .ok_or_else(|| WebError::internal("created queue could not be loaded"))?;

    Ok(Redirect::to(&format!("/sqs/queues/{}", created_queue.id)))
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

async fn dashboard_stats_fragment(
    State(state): State<WebState>,
    Query(params): Query<QueueListParams>,
) -> Result<Html<String>, WebError> {
    let summaries = load_queue_summaries(&state, &params).await?;
    let total_messages = summaries.iter().map(|queue| queue.message_count).sum();
    let visible_messages = summaries
        .iter()
        .map(|queue| queue.visible_message_count)
        .sum();
    let delayed_messages = summaries
        .iter()
        .map(|queue| queue.delayed_message_count)
        .sum();

    let template = SqsDashboardStatsTemplate {
        region: &params.region,
        account_id: &params.account_id,
        total_queues: summaries.len(),
        total_messages,
        visible_messages,
        delayed_messages,
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
    let tags = queue_tags(state.sqs_store.list_queue_tags(queue.id).await?);
    let queue_arn = queue_arn(&queue);

    let template = SqsQueueDetailTemplate {
        queue_id: queue.id,
        queue: &queue,
        queue_arn: &queue_arn,
        summary: &summary,
        attributes: &attributes,
        tags: &tags,
        messages: &message_summaries,
        tag_count: tags.len(),
        has_tags: !tags.is_empty(),
        has_messages: !message_summaries.is_empty(),
        message_limit,
    };

    Ok(Html(template.render()?))
}

async fn queue_detail_stats_fragment(
    State(state): State<WebState>,
    Path(queue_id): Path<i64>,
) -> Result<Html<String>, WebError> {
    let queue = load_queue_by_id(&state, queue_id).await?;
    let summary = queue_summary(&state, queue).await?;
    let template = QueueDetailStatsTemplate {
        queue_id,
        summary: &summary,
    };

    Ok(Html(template.render()?))
}

async fn queue_messages_fragment(
    State(state): State<WebState>,
    Path(queue_id): Path<i64>,
    Query(params): Query<QueueDetailParams>,
) -> Result<Html<String>, WebError> {
    load_queue_by_id(&state, queue_id).await?;
    let message_limit = params.message_limit.clamp(1, 500);
    let messages = state
        .sqs_store
        .list_messages(queue_id, message_limit)
        .await?;
    let message_summaries = messages
        .into_iter()
        .map(message_summary)
        .collect::<Vec<_>>();
    let template = QueueMessageListTemplate {
        queue_id,
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
    load_queue_by_id(&state, queue_id).await?;
    state.sqs_store.purge_queue(queue_id).await?;
    Ok(Redirect::to(&format!("/sqs/queues/{queue_id}")))
}

async fn send_message(
    State(state): State<WebState>,
    Path(queue_id): Path<i64>,
    Form(form): Form<SendMessageForm>,
) -> Result<Redirect, WebError> {
    let queue = load_queue_by_id(&state, queue_id).await?;
    if form.message_body.is_empty() {
        return Err(WebError::bad_request("Message body must not be empty"));
    }
    if form.message_body.len() > queue.maximum_message_size as usize {
        return Err(WebError::bad_request(format!(
            "Message body exceeds the queue maximum size of {} bytes",
            queue.maximum_message_size
        )));
    }

    let delay_seconds = parse_optional_i64(&form.delay_seconds, "DelaySeconds", 0, 900)?
        .unwrap_or(queue.delay_seconds);
    let now = Utc::now().naive_utc();
    let visible_at = now + Duration::seconds(delay_seconds);
    let expires_at = now + Duration::seconds(queue.message_retention_period_seconds);
    let message_attributes =
        normalize_optional_json_object(&form.message_attributes_json, "Message attributes JSON")?;

    let message = SqsMessage {
        message_id: uuid::Uuid::new_v4().to_string(),
        queue_id: queue.id,
        body: form.message_body,
        message_attributes,
        aws_trace_header: non_empty_trimmed(form.aws_trace_header),
        sent_at: now,
        visible_at,
        expires_at,
        receive_count: 0,
        receipt_handle: None,
        first_received_at: None,
        message_group_id: non_empty_trimmed(form.message_group_id),
        message_deduplication_id: non_empty_trimmed(form.message_deduplication_id),
    };

    state.sqs_store.send_message(&message).await?;

    Ok(Redirect::to(&format!("/sqs/queues/{queue_id}")))
}

async fn delete_queue(
    State(state): State<WebState>,
    Path(queue_id): Path<i64>,
) -> Result<Redirect, WebError> {
    let queue = load_queue_by_id(&state, queue_id).await?;
    state.sqs_store.delete_queue(queue_id).await?;
    Ok(Redirect::to(&format!(
        "/sqs/queues?region={}&account_id={}",
        queue.region, queue.account_id
    )))
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

async fn load_queue_by_id(state: &WebState, queue_id: i64) -> Result<SqsQueue, WebError> {
    state
        .sqs_store
        .get_queue_by_id(queue_id)
        .await?
        .ok_or_else(|| StoreError::NotFound("queue not found".to_string()).into())
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

fn queue_arn(queue: &SqsQueue) -> String {
    format!(
        "arn:aws:sqs:{}:{}:{}",
        queue.region, queue.account_id, queue.name
    )
}

fn validate_required(field_name: &str, value: &str) -> Result<(), WebError> {
    if value.is_empty() {
        return Err(WebError::bad_request(format!("{field_name} is required")));
    }

    Ok(())
}

fn validate_queue_name(queue_name: &str, queue_type: &str) -> Result<(), WebError> {
    let fifo_queue = match queue_type {
        "standard" => false,
        "fifo" => true,
        _ => {
            return Err(WebError::bad_request(
                "Queue type must be either standard or fifo",
            ));
        }
    };

    if queue_name.len() > 80 {
        return Err(WebError::bad_request(
            "Queue name must be at most 80 characters",
        ));
    }

    let name_without_fifo_suffix = queue_name.strip_suffix(".fifo").unwrap_or(queue_name);
    let valid_chars = name_without_fifo_suffix
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || ch == '-' || ch == '_');
    if !valid_chars {
        return Err(WebError::bad_request(
            "Queue name may only contain alphanumeric characters, hyphens, and underscores",
        ));
    }

    if fifo_queue && !queue_name.ends_with(".fifo") {
        return Err(WebError::bad_request(
            "FIFO queue names must end with .fifo",
        ));
    }

    if !fifo_queue && queue_name.ends_with(".fifo") {
        return Err(WebError::bad_request(
            "Queue names ending with .fifo must use the FIFO queue type",
        ));
    }

    Ok(())
}

fn parse_optional_i64(
    raw: &str,
    field_name: &str,
    min: i64,
    max: i64,
) -> Result<Option<i64>, WebError> {
    let raw = raw.trim();
    if raw.is_empty() {
        return Ok(None);
    }

    let value = raw.parse::<i64>().map_err(|_| {
        WebError::bad_request(format!(
            "{field_name} must be an integer between {min} and {max}"
        ))
    })?;

    if !(min..=max).contains(&value) {
        return Err(WebError::bad_request(format!(
            "{field_name} must be between {min} and {max}"
        )));
    }

    Ok(Some(value))
}

fn normalize_optional_json_object(raw: &str, field_name: &str) -> Result<Option<String>, WebError> {
    let raw = raw.trim();
    if raw.is_empty() {
        return Ok(None);
    }

    let value = serde_json::from_str::<serde_json::Value>(raw)
        .map_err(|e| WebError::bad_request(format!("{field_name} must be valid JSON: {e}")))?;

    if !value.is_object() {
        return Err(WebError::bad_request(format!(
            "{field_name} must be a JSON object"
        )));
    }

    if value.as_object().is_some_and(|object| object.is_empty()) {
        return Ok(None);
    }

    serde_json::to_string(&value)
        .map(Some)
        .map_err(|e| WebError::internal(format!("failed to serialize JSON: {e}")))
}

fn non_empty_trimmed(value: String) -> Option<String> {
    let value = value.trim().to_string();
    (!value.is_empty()).then_some(value)
}

fn queue_tags(tags: std::collections::HashMap<String, String>) -> Vec<QueueTag> {
    BTreeMap::from_iter(tags)
        .into_iter()
        .map(|(key, value)| QueueTag { key, value })
        .collect()
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
    use std::collections::HashMap;

    use hiraeth_store::sqs::SqsQueue;

    use super::{
        normalize_optional_json_object, parse_message_attributes, parse_optional_i64, queue_arn,
        queue_tags, validate_queue_name,
    };

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

    #[test]
    fn queue_arn_uses_queue_identity_fields() {
        let arn = queue_arn(&SqsQueue {
            name: "orders".to_string(),
            region: "us-east-1".to_string(),
            account_id: "000000000000".to_string(),
            ..Default::default()
        });

        assert_eq!(arn, "arn:aws:sqs:us-east-1:000000000000:orders");
    }

    #[test]
    fn queue_tags_are_sorted_for_stable_rendering() {
        let tags = queue_tags(HashMap::from([
            ("zeta".to_string(), "last".to_string()),
            ("alpha".to_string(), "first".to_string()),
        ]));

        assert_eq!(tags[0].key, "alpha");
        assert_eq!(tags[0].value, "first");
        assert_eq!(tags[1].key, "zeta");
        assert_eq!(tags[1].value, "last");
    }

    #[test]
    fn validate_queue_name_rejects_fifo_name_for_standard_queue() {
        let result = validate_queue_name("orders.fifo", "standard");

        assert!(result.is_err());
    }

    #[test]
    fn validate_queue_name_accepts_fifo_queue_with_fifo_suffix() {
        let result = validate_queue_name("orders.fifo", "fifo");

        assert!(result.is_ok());
    }

    #[test]
    fn parse_optional_i64_allows_blank_values() {
        let value = parse_optional_i64("", "DelaySeconds", 0, 900)
            .expect("blank optional number should parse");

        assert_eq!(value, None);
    }

    #[test]
    fn parse_optional_i64_rejects_out_of_range_values() {
        let result = parse_optional_i64("901", "DelaySeconds", 0, 900);

        assert!(result.is_err());
    }

    #[test]
    fn normalize_optional_json_object_minifies_json_objects() {
        let value = normalize_optional_json_object(
            r#"{
                "trace_id": {
                    "DataType": "String",
                    "StringValue": "abc123",
                    "BinaryValue": null
                }
            }"#,
            "Message attributes JSON",
        )
        .expect("message attributes should parse");

        assert_eq!(
            value.as_deref(),
            Some(r#"{"trace_id":{"BinaryValue":null,"DataType":"String","StringValue":"abc123"}}"#)
        );
    }
}
