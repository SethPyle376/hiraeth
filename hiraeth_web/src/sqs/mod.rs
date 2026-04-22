use std::collections::{BTreeMap, HashMap};

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
    components::{
        ActionCard, ActionCardAction, EmptyState, HeaderAction, MetadataEntry, MetadataList,
        PageHeader, StatBlock, StatBlockGrid,
    },
    error::WebError,
    templates::{
        QueueDetailStatsTemplate, QueueListTemplate, QueueMessageListTemplate,
        SqsDashboardStatsTemplate, SqsDashboardTemplate, SqsQueueDetailTemplate, SqsQueuesTemplate,
    },
};

const MAX_TAGS_PER_QUEUE: usize = 50;
const MAX_TAG_KEY_LENGTH: usize = 128;
const MAX_TAG_VALUE_LENGTH: usize = 256;

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
    #[serde(default)]
    feedback: Option<String>,
    #[serde(default)]
    feedback_kind: Option<String>,
    #[serde(default)]
    create_error: Option<String>,
    #[serde(default)]
    create_queue_name: Option<String>,
    #[serde(default)]
    create_queue_type: Option<String>,
    #[serde(default)]
    create_region: Option<String>,
    #[serde(default)]
    create_account_id: Option<String>,
}

fn default_message_limit() -> i64 {
    50
}

#[derive(Debug, Clone, Deserialize)]
struct QueueDetailParams {
    #[serde(default = "default_message_limit")]
    message_limit: i64,
    #[serde(default)]
    feedback: Option<String>,
    #[serde(default)]
    feedback_kind: Option<String>,
    #[serde(default)]
    send_error: Option<String>,
    #[serde(default)]
    tag_error: Option<String>,
    #[serde(default)]
    tag_key: Option<String>,
    #[serde(default)]
    tag_value: Option<String>,
    #[serde(default)]
    tags_open: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct CreateQueueForm {
    queue_name: String,
    region: String,
    account_id: String,
    queue_type: String,
    #[serde(default)]
    return_to: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct SendMessageForm {
    message_body: String,
    #[serde(default)]
    message_limit: String,
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

#[derive(Debug, Clone, Deserialize)]
struct TagQueueForm {
    tag_key: String,
    tag_value: String,
    #[serde(default)]
    message_limit: String,
}

#[derive(Debug, Clone, Deserialize)]
struct UntagQueueForm {
    tag_key: String,
    #[serde(default)]
    message_limit: String,
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

struct PageFeedback {
    message: String,
    alert_class: &'static str,
    has_message: bool,
}

struct CreateQueueFields {
    error: String,
    has_error: bool,
    queue_name: String,
    queue_type: String,
    region: String,
    account_id: String,
}

struct TagFormFields {
    error: String,
    has_error: bool,
    key: String,
    value: String,
    open_panel: bool,
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
        .route("/queues/{queue_id}/tags", post(tag_queue))
        .route("/queues/{queue_id}/tags/delete", post(untag_queue))
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
    let return_to = scoped_page_path("/sqs", &params);
    let feedback = feedback_from_params(&params.feedback_kind, &params.feedback);
    let create_fields = create_queue_fields(&params);
    let total_messages = summaries.iter().map(|queue| queue.message_count).sum();
    let visible_messages = summaries
        .iter()
        .map(|queue| queue.visible_message_count)
        .sum();
    let delayed_messages = summaries
        .iter()
        .map(|queue| queue.delayed_message_count)
        .sum();
    let page_header_html = dashboard_page_header(&params.region, &params.account_id)?;
    let dashboard_stats_html = dashboard_stats_html(
        &params.region,
        &params.account_id,
        summaries.len(),
        total_messages,
        visible_messages,
        delayed_messages,
    )?;
    let empty_state_html = queue_list_empty_state_html()?;

    let template = SqsDashboardTemplate {
        page_header_html: &page_header_html,
        dashboard_stats_html: &dashboard_stats_html,
        empty_state_html: &empty_state_html,
        region: &params.region,
        account_id: &params.account_id,
        prefix: &prefix,
        return_to: &return_to,
        feedback_message: &feedback.message,
        feedback_class: feedback.alert_class,
        has_feedback: feedback.has_message,
        create_error: &create_fields.error,
        has_create_error: create_fields.has_error,
        create_queue_name: &create_fields.queue_name,
        create_queue_type: &create_fields.queue_type,
        create_region: &create_fields.region,
        create_account_id: &create_fields.account_id,
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
    let return_to = scoped_page_path("/sqs/queues", &params);
    let feedback = feedback_from_params(&params.feedback_kind, &params.feedback);
    let create_fields = create_queue_fields(&params);
    let empty_state_html = queue_list_empty_state_html()?;
    let template = SqsQueuesTemplate {
        empty_state_html: &empty_state_html,
        region: &params.region,
        account_id: &params.account_id,
        prefix: &prefix,
        return_to: &return_to,
        feedback_message: &feedback.message,
        feedback_class: feedback.alert_class,
        has_feedback: feedback.has_message,
        create_error: &create_fields.error,
        has_create_error: create_fields.has_error,
        create_queue_name: &create_fields.queue_name,
        create_queue_type: &create_fields.queue_type,
        create_region: &create_fields.region,
        create_account_id: &create_fields.account_id,
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

    if let Err(error) = validate_required("Queue name", queue_name)
        .and_then(|_| validate_required("Region", region))
        .and_then(|_| validate_required("Account ID", account_id))
        .and_then(|_| validate_queue_name(queue_name, queue_type))
    {
        return Ok(create_queue_error_redirect(&form, error.message()));
    }

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

    if let Err(error) = state.sqs_store.create_queue(queue).await {
        if matches!(error, StoreError::Conflict(_)) {
            return Ok(create_queue_error_redirect(&form, &error.to_string()));
        }

        return Err(error.into());
    }

    let created_queue = state
        .sqs_store
        .get_queue(queue_name, region, account_id)
        .await?
        .ok_or_else(|| WebError::internal("created queue could not be loaded"))?;

    Ok(feedback_redirect(
        format!("/sqs/queues/{}", created_queue.id),
        "success",
        &format!("Created queue {queue_name}."),
    ))
}

async fn queues_fragment(
    State(state): State<WebState>,
    Query(params): Query<QueueListParams>,
) -> Result<Html<String>, WebError> {
    let summaries = load_queue_summaries(&state, &params).await?;
    let empty_state_html = queue_list_empty_state_html()?;
    let template = QueueListTemplate {
        empty_state_html: &empty_state_html,
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
    let stats_cards_html = dashboard_stats_cards_html(
        &params.region,
        &params.account_id,
        summaries.len(),
        total_messages,
        visible_messages,
        delayed_messages,
    )?;

    let template = SqsDashboardStatsTemplate {
        stats_cards_html: &stats_cards_html,
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
    let queue_url = queue_url(&state, &queue);
    let feedback = feedback_from_params(&params.feedback_kind, &params.feedback);
    let send_error = params.send_error.clone().unwrap_or_default();
    let tag_fields = tag_form_fields(&params);
    let action_card_html = queue_action_card_html(&queue, message_limit)?;
    let metadata_list_html = queue_metadata_list_html(&queue)?;

    let template = SqsQueueDetailTemplate {
        action_card_html: &action_card_html,
        metadata_list_html: &metadata_list_html,
        queue_id: queue.id,
        queue: &queue,
        queue_arn: &queue_arn,
        queue_url: &queue_url,
        feedback_message: &feedback.message,
        feedback_class: feedback.alert_class,
        has_feedback: feedback.has_message,
        send_error: &send_error,
        has_send_error: !send_error.is_empty(),
        tag_error: &tag_fields.error,
        has_tag_error: tag_fields.has_error,
        tag_key: &tag_fields.key,
        tag_value: &tag_fields.value,
        open_tags_panel: tag_fields.open_panel,
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
    Query(params): Query<QueueDetailParams>,
) -> Result<Redirect, WebError> {
    load_queue_by_id(&state, queue_id).await?;
    state.sqs_store.purge_queue(queue_id).await?;
    let message_limit = params.message_limit.clamp(1, 500).to_string();
    Ok(feedback_redirect(
        queue_detail_path(queue_id, &message_limit),
        "success",
        "Purged queued messages.",
    ))
}

async fn send_message(
    State(state): State<WebState>,
    Path(queue_id): Path<i64>,
    Form(form): Form<SendMessageForm>,
) -> Result<Redirect, WebError> {
    let queue = load_queue_by_id(&state, queue_id).await?;
    if form.message_body.is_empty() {
        return Ok(send_message_error_redirect(
            queue_id,
            &form.message_limit,
            "Message body must not be empty",
        ));
    }
    if form.message_body.len() > queue.maximum_message_size as usize {
        return Ok(send_message_error_redirect(
            queue_id,
            &form.message_limit,
            &format!(
                "Message body exceeds the queue maximum size of {} bytes",
                queue.maximum_message_size
            ),
        ));
    }

    let delay_seconds = match parse_optional_i64(&form.delay_seconds, "DelaySeconds", 0, 900) {
        Ok(value) => value.unwrap_or(queue.delay_seconds),
        Err(error) => {
            return Ok(send_message_error_redirect(
                queue_id,
                &form.message_limit,
                error.message(),
            ));
        }
    };
    let now = Utc::now().naive_utc();
    let visible_at = now + Duration::seconds(delay_seconds);
    let expires_at = now + Duration::seconds(queue.message_retention_period_seconds);
    let message_attributes = match normalize_optional_json_object(
        &form.message_attributes_json,
        "Message attributes JSON",
    ) {
        Ok(value) => value,
        Err(error) => {
            return Ok(send_message_error_redirect(
                queue_id,
                &form.message_limit,
                error.message(),
            ));
        }
    };

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

    Ok(feedback_redirect(
        queue_detail_path(queue_id, &form.message_limit),
        "success",
        "Sent message.",
    ))
}

async fn tag_queue(
    State(state): State<WebState>,
    Path(queue_id): Path<i64>,
    Form(form): Form<TagQueueForm>,
) -> Result<Redirect, WebError> {
    load_queue_by_id(&state, queue_id).await?;
    let tag_key = form.tag_key.trim();
    let tag_value = form.tag_value.as_str();

    if let Err(error) =
        validate_tag_key(tag_key).and_then(|_| validate_tag_value(tag_key, tag_value))
    {
        return Ok(tag_queue_error_redirect(queue_id, &form, error.message()));
    }

    let existing_tags = state.sqs_store.list_queue_tags(queue_id).await?;
    if !existing_tags.contains_key(tag_key) && existing_tags.len() >= MAX_TAGS_PER_QUEUE {
        return Ok(tag_queue_error_redirect(
            queue_id,
            &form,
            &format!("A queue can have at most {MAX_TAGS_PER_QUEUE} tags"),
        ));
    }

    state
        .sqs_store
        .tag_queue(
            queue_id,
            HashMap::from([(tag_key.to_string(), tag_value.to_string())]),
        )
        .await?;

    Ok(feedback_redirect(
        queue_tags_path(queue_id, &form.message_limit),
        "success",
        &format!("Saved tag {tag_key}."),
    ))
}

async fn untag_queue(
    State(state): State<WebState>,
    Path(queue_id): Path<i64>,
    Form(form): Form<UntagQueueForm>,
) -> Result<Redirect, WebError> {
    load_queue_by_id(&state, queue_id).await?;
    let tag_key = form.tag_key.trim();
    if let Err(error) = validate_tag_key(tag_key) {
        return Ok(feedback_redirect(
            queue_tags_path(queue_id, &form.message_limit),
            "error",
            error.message(),
        ));
    }

    state
        .sqs_store
        .untag_queue(queue_id, vec![tag_key.to_string()])
        .await?;

    Ok(feedback_redirect(
        queue_tags_path(queue_id, &form.message_limit),
        "success",
        &format!("Removed tag {tag_key}."),
    ))
}

async fn delete_queue(
    State(state): State<WebState>,
    Path(queue_id): Path<i64>,
) -> Result<Redirect, WebError> {
    let queue = load_queue_by_id(&state, queue_id).await?;
    state.sqs_store.delete_queue(queue_id).await?;
    Ok(feedback_redirect(
        format!(
            "/sqs/queues?region={}&account_id={}",
            queue.region, queue.account_id
        ),
        "success",
        &format!("Deleted queue {}.", queue.name),
    ))
}

async fn delete_message(
    State(state): State<WebState>,
    Path((queue_id, message_id)): Path<(i64, String)>,
    Query(params): Query<QueueDetailParams>,
) -> Result<Redirect, WebError> {
    state
        .sqs_store
        .delete_message_by_id(queue_id, &message_id)
        .await?;
    let message_limit = params.message_limit.clamp(1, 500).to_string();
    Ok(feedback_redirect(
        queue_detail_path(queue_id, &message_limit),
        "success",
        "Deleted message.",
    ))
}

fn feedback_from_params(kind: &Option<String>, message: &Option<String>) -> PageFeedback {
    let message = message.clone().unwrap_or_default();
    let alert_class = match kind.as_deref() {
        Some("error") => "alert-error",
        Some("warning") => "alert-warning",
        _ => "alert-success",
    };

    PageFeedback {
        has_message: !message.is_empty(),
        message,
        alert_class,
    }
}

fn create_queue_fields(params: &QueueListParams) -> CreateQueueFields {
    let error = params.create_error.clone().unwrap_or_default();
    let queue_type = params
        .create_queue_type
        .as_deref()
        .filter(|value| matches!(*value, "standard" | "fifo"))
        .unwrap_or("standard")
        .to_string();

    CreateQueueFields {
        has_error: !error.is_empty(),
        error,
        queue_name: params.create_queue_name.clone().unwrap_or_default(),
        queue_type,
        region: params
            .create_region
            .clone()
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| params.region.clone()),
        account_id: params
            .create_account_id
            .clone()
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| params.account_id.clone()),
    }
}

fn tag_form_fields(params: &QueueDetailParams) -> TagFormFields {
    let error = params.tag_error.clone().unwrap_or_default();
    let open_panel = !error.is_empty() || params.tags_open.as_deref() == Some("1");

    TagFormFields {
        has_error: !error.is_empty(),
        error,
        key: params.tag_key.clone().unwrap_or_default(),
        value: params.tag_value.clone().unwrap_or_default(),
        open_panel,
    }
}

fn scoped_page_path(base_path: &str, params: &QueueListParams) -> String {
    let prefix = params.prefix.clone().unwrap_or_default();
    append_query_params(
        base_path.to_string(),
        &[
            ("region", params.region.as_str()),
            ("account_id", params.account_id.as_str()),
            ("prefix", prefix.as_str()),
        ],
    )
}

fn create_queue_error_redirect(form: &CreateQueueForm, message: &str) -> Redirect {
    let return_to = safe_return_to(form.return_to.as_deref(), "/sqs/queues");
    Redirect::to(&append_query_params(
        return_to,
        &[
            ("create_error", message),
            ("create_queue_name", form.queue_name.trim()),
            ("create_queue_type", form.queue_type.trim()),
            ("create_region", form.region.trim()),
            ("create_account_id", form.account_id.trim()),
        ],
    ))
}

fn send_message_error_redirect(queue_id: i64, message_limit: &str, message: &str) -> Redirect {
    Redirect::to(&append_query_params(
        queue_detail_path(queue_id, message_limit),
        &[("send_error", message)],
    ))
}

fn tag_queue_error_redirect(queue_id: i64, form: &TagQueueForm, message: &str) -> Redirect {
    Redirect::to(&append_query_params(
        queue_tags_path(queue_id, &form.message_limit),
        &[
            ("tag_error", message),
            ("tag_key", form.tag_key.trim()),
            ("tag_value", form.tag_value.as_str()),
        ],
    ))
}

fn queue_detail_path(queue_id: i64, message_limit: &str) -> String {
    append_query_params(
        format!("/sqs/queues/{queue_id}"),
        &[("message_limit", message_limit)],
    )
}

fn queue_tags_path(queue_id: i64, message_limit: &str) -> String {
    append_query_params(
        queue_detail_path(queue_id, message_limit),
        &[("tags_open", "1")],
    )
}

fn feedback_redirect(path: String, kind: &str, message: &str) -> Redirect {
    Redirect::to(&append_query_params(
        path,
        &[("feedback_kind", kind), ("feedback", message)],
    ))
}

fn safe_return_to(return_to: Option<&str>, fallback: &str) -> String {
    let Some(return_to) = return_to else {
        return fallback.to_string();
    };

    let valid_sqs_path =
        return_to == "/sqs" || return_to.starts_with("/sqs/") || return_to.starts_with("/sqs?");
    if valid_sqs_path && !return_to.starts_with("//") && !return_to.contains(['\n', '\r']) {
        return return_to.to_string();
    }

    fallback.to_string()
}

fn append_query_params(mut path: String, params: &[(&str, &str)]) -> String {
    let mut first = !path.contains('?');
    for (key, value) in params {
        if value.is_empty() {
            continue;
        }

        path.push(if first { '?' } else { '&' });
        first = false;
        path.push_str(&urlencoding::encode(key));
        path.push('=');
        path.push_str(&urlencoding::encode(value));
    }

    path
}

fn dashboard_page_header(region: &str, account_id: &str) -> Result<String, askama::Error> {
    PageHeader {
        eyebrow: "Service Dashboard".to_string(),
        title: "SQS".to_string(),
        description: "Inspect queue state for one account and region. Use the AWS SDK or CLI to create resources; use this page to verify what the emulator stored.".to_string(),
        actions: vec![
            HeaderAction::link(
                "Browse queues",
                format!("/sqs/queues?region={region}&account_id={account_id}"),
                "btn btn-primary",
            ),
            HeaderAction::link(
                "JSON API",
                format!("/sqs/api/queues?region={region}&account_id={account_id}"),
                "btn btn-outline",
            ),
        ],
    }
    .render()
}

fn dashboard_stats_html(
    region: &str,
    account_id: &str,
    total_queues: usize,
    total_messages: i64,
    visible_messages: i64,
    delayed_messages: i64,
) -> Result<String, askama::Error> {
    Ok(format!(
        r##"<div id="dashboard-stats"
     class="mb-5"
     hx-get="/sqs/fragments/dashboard-stats"
     hx-trigger="every 3s"
     hx-include="#dashboard-queue-filter"
     hx-swap="outerHTML">{}</div>"##,
        dashboard_stats_cards_html(
            region,
            account_id,
            total_queues,
            total_messages,
            visible_messages,
            delayed_messages,
        )?
    ))
}

fn dashboard_stats_cards_html(
    region: &str,
    account_id: &str,
    total_queues: usize,
    total_messages: i64,
    visible_messages: i64,
    delayed_messages: i64,
) -> Result<String, askama::Error> {
    StatBlockGrid {
        grid_class: "grid gap-3 md:grid-cols-4",
        blocks: vec![
            StatBlock {
                title: "Queues".to_string(),
                value: total_queues.to_string(),
                value_class: "text-primary",
                description: format!("{account_id} / {region}"),
            },
            StatBlock {
                title: "Messages".to_string(),
                value: total_messages.to_string(),
                value_class: "text-secondary",
                description: "retained and not expired".to_string(),
            },
            StatBlock {
                title: "Visible".to_string(),
                value: visible_messages.to_string(),
                value_class: "text-success",
                description: "available to consumers".to_string(),
            },
            StatBlock {
                title: "Delayed".to_string(),
                value: delayed_messages.to_string(),
                value_class: "text-info",
                description: "waiting for delay".to_string(),
            },
        ],
    }
    .render()
}

fn queue_list_empty_state_html() -> Result<String, askama::Error> {
    EmptyState {
        title: "No queues found".to_string(),
        message: "Create a queue through the AWS emulator, or adjust the account, region, or prefix filter.".to_string(),
    }
    .render()
}

fn queue_action_card_html(queue: &SqsQueue, message_limit: i64) -> Result<String, askama::Error> {
    ActionCard {
        title: "Queue actions".to_string(),
        grid_class: "sm:grid-cols-4",
        actions: vec![
            ActionCardAction::link(
                "Back",
                format!(
                    "/sqs/queues?region={}&account_id={}",
                    queue.region, queue.account_id
                ),
                "btn btn-ghost",
            ),
            ActionCardAction::link(
                "Reload",
                format!("/sqs/queues/{}?message_limit={message_limit}", queue.id),
                "btn btn-outline",
            ),
            ActionCardAction::post(
                "Purge",
                format!(
                    "/sqs/queues/{}/purge?message_limit={message_limit}",
                    queue.id
                ),
                "btn btn-warning w-full",
                Some(
                    "return hiraethConfirmSubmit(this, 'Purge every message in this queue?', 'Purging...')"
                        .to_string(),
                ),
            ),
            ActionCardAction::post(
                "Delete",
                format!("/sqs/queues/{}/delete", queue.id),
                "btn btn-error w-full",
                Some(
                    "return hiraethConfirmSubmit(this, 'Delete this queue and all messages?', 'Deleting...')"
                        .to_string(),
                ),
            ),
        ],
    }
    .render()
}

fn queue_metadata_list_html(queue: &SqsQueue) -> Result<String, askama::Error> {
    MetadataList {
        entries: vec![
            MetadataEntry {
                label: "Database ID".to_string(),
                value: queue.id.to_string(),
                value_class: "font-mono",
            },
            MetadataEntry {
                label: "Created".to_string(),
                value: queue.created_at.to_string(),
                value_class: "font-mono",
            },
            MetadataEntry {
                label: "Updated".to_string(),
                value: queue.updated_at.to_string(),
                value_class: "font-mono",
            },
        ],
    }
    .render()
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

fn queue_url(state: &WebState, queue: &SqsQueue) -> String {
    format!(
        "{}/{}/{}",
        state.aws_endpoint_url.trim_end_matches('/'),
        queue.account_id,
        queue.name
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

fn validate_tag_key(key: &str) -> Result<(), WebError> {
    let key_length = key.chars().count();

    if key_length == 0 || key_length > MAX_TAG_KEY_LENGTH {
        return Err(WebError::bad_request(format!(
            "Tag keys must be between 1 and {MAX_TAG_KEY_LENGTH} characters"
        )));
    }

    if key.starts_with("aws:") {
        return Err(WebError::bad_request(
            "Tag keys cannot start with the reserved aws: prefix",
        ));
    }

    Ok(())
}

fn validate_tag_value(key: &str, value: &str) -> Result<(), WebError> {
    if value.chars().count() > MAX_TAG_VALUE_LENGTH {
        return Err(WebError::bad_request(format!(
            "Tag value for '{key}' must be at most {MAX_TAG_VALUE_LENGTH} characters"
        )));
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

fn queue_tags(tags: HashMap<String, String>) -> Vec<QueueTag> {
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
        append_query_params, feedback_from_params, normalize_optional_json_object,
        parse_message_attributes, parse_optional_i64, queue_arn, queue_tags, safe_return_to,
        validate_queue_name, validate_tag_key, validate_tag_value,
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

    #[test]
    fn append_query_params_encodes_values_and_preserves_existing_query() {
        let path = append_query_params(
            "/sqs/queues?region=us-east-1".to_string(),
            &[
                ("feedback", "Created queue orders & billing."),
                ("empty", ""),
            ],
        );

        assert_eq!(
            path,
            "/sqs/queues?region=us-east-1&feedback=Created%20queue%20orders%20%26%20billing."
        );
    }

    #[test]
    fn safe_return_to_rejects_external_paths() {
        assert_eq!(
            safe_return_to(Some("https://example.com"), "/sqs/queues"),
            "/sqs/queues"
        );
        assert_eq!(
            safe_return_to(Some("/sqs.evil"), "/sqs/queues"),
            "/sqs/queues"
        );
        assert_eq!(
            safe_return_to(Some("/sqs?region=test"), "/sqs/queues"),
            "/sqs?region=test"
        );
    }

    #[test]
    fn feedback_from_params_defaults_to_success_class() {
        let feedback = feedback_from_params(&None, &Some("Created queue.".to_string()));

        assert!(feedback.has_message);
        assert_eq!(feedback.alert_class, "alert-success");
        assert_eq!(feedback.message, "Created queue.");
    }

    #[test]
    fn validate_tag_key_rejects_reserved_prefix() {
        let result = validate_tag_key("aws:reserved");

        assert!(result.is_err());
    }

    #[test]
    fn validate_tag_key_accepts_non_reserved_key() {
        let result = validate_tag_key("environment");

        assert!(result.is_ok());
    }

    #[test]
    fn validate_tag_value_rejects_values_over_256_chars() {
        let value = "x".repeat(257);
        let result = validate_tag_value("environment", &value);

        assert!(result.is_err());
    }
}
