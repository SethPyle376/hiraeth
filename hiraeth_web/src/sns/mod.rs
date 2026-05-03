use std::collections::BTreeMap;

use askama::Template;
use axum::{
    Json, Router,
    extract::{Form, Path, Query, State},
    response::{Html, Redirect},
    routing::{get, post},
};
use chrono::Utc;
use hiraeth_store::{
    StoreError,
    sns::{SnsStore, SnsSubscription, SnsTopic},
    sqs::{SqsMessage, SqsStore},
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
        SnsDashboardStatsTemplate, SnsDashboardTemplate, SnsSubscriptionListTemplate,
        SnsTopicDetailTemplate, SnsTopicListTemplate, SnsTopicsTemplate,
    },
};

fn default_region() -> String {
    "us-east-1".to_string()
}

fn default_account_id() -> String {
    "000000000000".to_string()
}

#[derive(Debug, Clone, Deserialize)]
struct TopicListParams {
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
    create_topic_name: Option<String>,
    #[serde(default)]
    create_region: Option<String>,
    #[serde(default)]
    create_account_id: Option<String>,
    #[serde(default)]
    create_display_name: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct TopicDetailParams {
    #[serde(default)]
    feedback: Option<String>,
    #[serde(default)]
    feedback_kind: Option<String>,
    #[serde(default)]
    subscription_error: Option<String>,
    #[serde(default)]
    subscription_protocol: Option<String>,
    #[serde(default)]
    subscription_endpoint: Option<String>,
    #[serde(default)]
    subscription_raw_message_delivery: Option<String>,
    #[serde(default)]
    publish_error: Option<String>,
    #[serde(default)]
    publish_message: Option<String>,
    #[serde(default)]
    publish_subject: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct CreateTopicForm {
    name: String,
    region: String,
    account_id: String,
    #[serde(default)]
    display_name: String,
    #[serde(default)]
    return_to: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct CreateSubscriptionForm {
    protocol: String,
    endpoint: String,
    #[serde(default)]
    raw_message_delivery: String,
}

#[derive(Debug, Clone, Deserialize)]
struct PublishForm {
    message: String,
    #[serde(default)]
    subject: String,
}

#[derive(Debug, Clone, Serialize)]
struct TopicListResponse {
    region: String,
    account_id: String,
    topics: Vec<TopicSummary>,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct TopicSummary {
    pub(crate) id: i64,
    pub(crate) name: String,
    pub(crate) region: String,
    pub(crate) account_id: String,
    pub(crate) display_name: Option<String>,
    pub(crate) subscription_count: usize,
}

#[derive(Debug, Clone)]
pub(crate) struct SubscriptionSummary {
    pub(crate) id: i64,
    pub(crate) protocol: String,
    pub(crate) endpoint: String,
    pub(crate) subscription_arn: String,
    pub(crate) raw_message_delivery: bool,
    pub(crate) created_at: String,
}

struct PageFeedback {
    message: String,
    alert_class: &'static str,
    has_message: bool,
}

struct CreateTopicFields {
    error: String,
    has_error: bool,
    topic_name: String,
    region: String,
    account_id: String,
    display_name: String,
}

struct SubscriptionFormFields {
    error: String,
    has_error: bool,
    protocol: String,
    endpoint: String,
    raw_message_delivery: bool,
}

struct PublishFormFields {
    error: String,
    has_error: bool,
    message: String,
    subject: String,
}

pub fn router() -> Router<WebState> {
    Router::new()
        .route("/", get(dashboard))
        .route("/topics", get(topics_page))
        .route("/topics/create", post(create_topic))
        .route("/topics/{topic_id}", get(topic_detail))
        .route("/topics/{topic_id}/delete", post(delete_topic))
        .route(
            "/topics/{topic_id}/subscriptions",
            post(create_subscription),
        )
        .route(
            "/topics/{topic_id}/subscriptions/{subscription_id}/delete",
            post(delete_subscription),
        )
        .route("/topics/{topic_id}/publish", post(publish_message))
        .route("/fragments/topics", get(topics_fragment))
        .route("/fragments/dashboard-stats", get(dashboard_stats_fragment))
        .route("/api/topics", get(topics_api))
}

async fn dashboard(
    State(state): State<WebState>,
    Query(params): Query<TopicListParams>,
) -> Result<Html<String>, WebError> {
    let summaries = load_topic_summaries(&state, &params).await?;
    let prefix = params.prefix.clone().unwrap_or_default();
    let return_to = scoped_page_path("/sns", &params);
    let feedback = feedback_from_params(&params.feedback_kind, &params.feedback);
    let create_fields = create_topic_fields(&params);
    let total_subscriptions = summaries
        .iter()
        .map(|topic| topic.subscription_count)
        .sum::<usize>();
    let page_header_html = dashboard_page_header(&params.region, &params.account_id)?;
    let dashboard_stats_html =
        dashboard_stats_html(&params.region, &params.account_id, summaries.len(), total_subscriptions)?;
    let empty_state_html = topic_list_empty_state_html()?;

    let template = SnsDashboardTemplate {
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
        create_topic_name: &create_fields.topic_name,
        create_region: &create_fields.region,
        create_account_id: &create_fields.account_id,
        create_display_name: &create_fields.display_name,
        topics: &summaries,
        has_topics: !summaries.is_empty(),
    };

    Ok(Html(template.render()?))
}

async fn topics_page(
    State(state): State<WebState>,
    Query(params): Query<TopicListParams>,
) -> Result<Html<String>, WebError> {
    let summaries = load_topic_summaries(&state, &params).await?;
    let prefix = params.prefix.clone().unwrap_or_default();
    let return_to = scoped_page_path("/sns/topics", &params);
    let feedback = feedback_from_params(&params.feedback_kind, &params.feedback);
    let create_fields = create_topic_fields(&params);
    let empty_state_html = topic_list_empty_state_html()?;
    let template = SnsTopicsTemplate {
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
        create_topic_name: &create_fields.topic_name,
        create_region: &create_fields.region,
        create_account_id: &create_fields.account_id,
        create_display_name: &create_fields.display_name,
        topics: &summaries,
        has_topics: !summaries.is_empty(),
    };

    Ok(Html(template.render()?))
}

async fn create_topic(
    State(state): State<WebState>,
    Form(form): Form<CreateTopicForm>,
) -> Result<Redirect, WebError> {
    let name = form.name.trim();
    let region = form.region.trim();
    let account_id = form.account_id.trim();
    let display_name = form.display_name.trim();
    let display_name = if display_name.is_empty() {
        None
    } else {
        Some(display_name.to_string())
    };

    if let Err(error) = validate_required("Topic name", name)
        .and_then(|_| validate_required("Region", region))
        .and_then(|_| validate_required("Account ID", account_id))
        .and_then(|_| validate_topic_name(name))
    {
        return Ok(create_topic_error_redirect(&form, error.message()));
    }

    let now = Utc::now().naive_utc();
    let topic = SnsTopic {
        id: 0,
        name: name.to_string(),
        region: region.to_string(),
        account_id: account_id.to_string(),
        display_name,
        policy: "{}".to_string(),
        delivery_policy: None,
        fifo_topic: None,
        signature_version: None,
        tracing_config: None,
        kms_master_key_id: None,
        data_protection_policy: None,
        created_at: now,
    };

    if let Err(error) = state.sns_store.create_topic(topic).await {
        if matches!(error, StoreError::Conflict(_)) {
            return Ok(create_topic_error_redirect(&form, &error.to_string()));
        }
        return Err(error.into());
    }

    let arn = topic_arn(name, region, account_id);
    let created_topic = state
        .sns_store
        .get_topic(&arn)
        .await?
        .ok_or_else(|| WebError::internal("created topic could not be loaded"))?;

    Ok(feedback_redirect(
        format!("/sns/topics/{}", created_topic.id),
        "success",
        &format!("Created topic {name}."),
    ))
}

async fn topics_fragment(
    State(state): State<WebState>,
    Query(params): Query<TopicListParams>,
) -> Result<Html<String>, WebError> {
    let summaries = load_topic_summaries(&state, &params).await?;
    let empty_state_html = topic_list_empty_state_html()?;
    let template = SnsTopicListTemplate {
        empty_state_html: &empty_state_html,
        topics: &summaries,
        has_topics: !summaries.is_empty(),
    };

    Ok(Html(template.render()?))
}

async fn dashboard_stats_fragment(
    State(state): State<WebState>,
    Query(params): Query<TopicListParams>,
) -> Result<Html<String>, WebError> {
    let summaries = load_topic_summaries(&state, &params).await?;
    let total_subscriptions = summaries
        .iter()
        .map(|topic| topic.subscription_count)
        .sum::<usize>();
    let stats_cards_html = dashboard_stats_cards_html(
        &params.region,
        &params.account_id,
        summaries.len(),
        total_subscriptions,
    )?;

    let template = SnsDashboardStatsTemplate {
        stats_cards_html: &stats_cards_html,
    };

    Ok(Html(template.render()?))
}

async fn topics_api(
    State(state): State<WebState>,
    Query(params): Query<TopicListParams>,
) -> Result<Json<TopicListResponse>, WebError> {
    let summaries = load_topic_summaries(&state, &params).await?;
    Ok(Json(TopicListResponse {
        region: params.region,
        account_id: params.account_id,
        topics: summaries,
    }))
}

async fn topic_detail(
    State(state): State<WebState>,
    Path(topic_id): Path<i64>,
    Query(params): Query<TopicDetailParams>,
) -> Result<Html<String>, WebError> {
    let topic = load_topic_by_id(&state, topic_id).await?;
    let summary = topic_summary(&state, topic.clone()).await?;
    let subscriptions = state
        .sns_store
        .list_subscriptions_by_topic(&topic_arn(&topic.name, &topic.region, &topic.account_id))
        .await?;
    let subscription_summaries = subscriptions
        .into_iter()
        .map(subscription_summary)
        .collect::<Vec<_>>();
    let arn = topic_arn(&topic.name, &topic.region, &topic.account_id);
    let feedback = feedback_from_params(&params.feedback_kind, &params.feedback);
    let subscription_fields = subscription_form_fields(&params);
    let publish_fields = publish_form_fields(&params);
    let action_card_html = topic_action_card_html(&topic)?;
    let metadata_list_html = topic_metadata_list_html(&topic)?;

    let template = SnsTopicDetailTemplate {
        action_card_html: &action_card_html,
        metadata_list_html: &metadata_list_html,
        topic_id: topic.id,
        topic: &topic,
        topic_arn: &arn,
        feedback_message: &feedback.message,
        feedback_class: feedback.alert_class,
        has_feedback: feedback.has_message,
        subscription_error: &subscription_fields.error,
        has_subscription_error: subscription_fields.has_error,
        subscription_protocol: &subscription_fields.protocol,
        subscription_endpoint: &subscription_fields.endpoint,
        subscription_raw_message_delivery: subscription_fields.raw_message_delivery,
        publish_error: &publish_fields.error,
        has_publish_error: publish_fields.has_error,
        publish_message: &publish_fields.message,
        publish_subject: &publish_fields.subject,
        summary: &summary,
        subscriptions: &subscription_summaries,
        subscription_count: subscription_summaries.len(),
        has_subscriptions: !subscription_summaries.is_empty(),
    };

    Ok(Html(template.render()?))
}

async fn create_subscription(
    State(state): State<WebState>,
    Path(topic_id): Path<i64>,
    Form(form): Form<CreateSubscriptionForm>,
) -> Result<Redirect, WebError> {
    let topic = load_topic_by_id(&state, topic_id).await?;
    let protocol = form.protocol.trim();
    let endpoint = form.endpoint.trim();

    let raw_message_delivery = form.raw_message_delivery.trim() == "on";

    if let Err(error) = validate_required("Protocol", protocol)
        .and_then(|_| validate_required("Endpoint", endpoint))
    {
        return Ok(subscription_error_redirect(
            topic_id,
            protocol,
            endpoint,
            raw_message_delivery,
            error.message(),
        ));
    }

    if protocol != "sqs" {
        return Ok(subscription_error_redirect(
            topic_id,
            protocol,
            endpoint,
            raw_message_delivery,
            "Only the 'sqs' protocol is currently supported",
        ));
    }

    let arn = topic_arn(&topic.name, &topic.region, &topic.account_id);
    let subscription_arn = format!(
        "arn:aws:sns:{}:{}:{}:{}",
        topic.region,
        topic.account_id,
        topic.name,
        uuid::Uuid::new_v4()
    );

    let subscription = SnsSubscription {
        id: 0,
        topic_arn: arn,
        protocol: protocol.to_string(),
        endpoint: endpoint.to_string(),
        owner_account_id: topic.account_id.clone(),
        subscription_arn,
        delivery_policy: None,
        filter_policy: None,
        filter_policy_scope: None,
        raw_message_delivery: if raw_message_delivery {
            Some("true".to_string())
        } else {
            None
        },
        redrive_policy: None,
        subscription_role_arn: None,
        replay_policy: None,
        created_at: Utc::now().naive_utc(),
    };

    state.sns_store.create_subscription(subscription).await?;

    Ok(feedback_redirect(
        topic_detail_path(topic_id),
        "success",
        &format!("Created {protocol} subscription."),
    ))
}

async fn delete_subscription(
    State(state): State<WebState>,
    Path((topic_id, subscription_id)): Path<(i64, i64)>,
) -> Result<Redirect, WebError> {
    load_topic_by_id(&state, topic_id).await?;
    state.sns_store.delete_subscription_by_id(subscription_id).await?;
    Ok(feedback_redirect(
        topic_detail_path(topic_id),
        "success",
        "Deleted subscription.",
    ))
}

async fn delete_topic(
    State(state): State<WebState>,
    Path(topic_id): Path<i64>,
) -> Result<Redirect, WebError> {
    let topic = load_topic_by_id(&state, topic_id).await?;
    let arn = topic_arn(&topic.name, &topic.region, &topic.account_id);
    state.sns_store.delete_topic(&arn).await?;
    Ok(feedback_redirect(
        format!(
            "/sns/topics?region={}&account_id={}",
            topic.region, topic.account_id
        ),
        "success",
        &format!("Deleted topic {}.", topic.name),
    ))
}

async fn publish_message(
    State(state): State<WebState>,
    Path(topic_id): Path<i64>,
    Form(form): Form<PublishForm>,
) -> Result<Redirect, WebError> {
    let topic = load_topic_by_id(&state, topic_id).await?;
    let message = form.message.trim();
    let subject = form.subject.trim();

    if let Err(error) = validate_required("Message", message) {
        return Ok(publish_error_redirect(topic_id, message, subject, error.message()));
    }

    let arn = topic_arn(&topic.name, &topic.region, &topic.account_id);
    let subscriptions = state.sns_store.list_subscriptions_by_topic(&arn).await?;
    let mut delivered = 0;
    let mut failed = 0;
    let message_id = uuid::Uuid::new_v4().to_string();

    for subscription in &subscriptions {
        if subscription.protocol != "sqs" {
            continue;
        }
        match deliver_to_sqs(
            &state,
            subscription,
            &topic,
            message,
            subject,
            &message_id,
        )
        .await
        {
            Ok(()) => delivered += 1,
            Err(_error) => {
                failed += 1;
            }
        }
    }

    let feedback_message = if delivered > 0 && failed == 0 {
        format!("Published message to {delivered} subscription(s).")
    } else if delivered > 0 && failed > 0 {
        format!("Published to {delivered} subscription(s), {failed} failed.")
    } else if subscriptions.is_empty() {
        "Published message. No subscriptions to deliver to.".to_string()
    } else {
        format!("Failed to deliver to all {failed} subscription(s).")
    };

    Ok(feedback_redirect(
        topic_detail_path(topic_id),
        if failed > 0 && delivered == 0 {
            "warning"
        } else {
            "success"
        },
        &feedback_message,
    ))
}

async fn deliver_to_sqs(
    state: &WebState,
    subscription: &SnsSubscription,
    topic: &SnsTopic,
    message: &str,
    subject: &str,
    message_id: &str,
) -> Result<(), WebError> {
    let queue_id = parse_sqs_endpoint_arn(&subscription.endpoint)
        .ok_or_else(|| WebError::bad_request(format!("Invalid SQS endpoint: {}", subscription.endpoint)))?;

    let sqs_queue = state
        .sqs_store
        .get_queue(&queue_id.name, &queue_id.region, &queue_id.account_id)
        .await?
        .ok_or_else(|| WebError::bad_request(format!("SQS queue not found: {}", subscription.endpoint)))?;

    let raw_delivery = subscription.raw_message_delivery.as_deref() == Some("true");

    let body = if raw_delivery {
        message.to_string()
    } else {
        serde_json::json!({
            "Type": "Notification",
            "MessageId": message_id,
            "TopicArn": format!("arn:aws:sns:{}:{}:{}", topic.region, topic.account_id, topic.name),
            "Subject": subject,
            "Message": message,
            "Timestamp": Utc::now().to_rfc3339(),
        })
        .to_string()
    };

    let now = Utc::now().naive_utc();
    let visible_at = now + chrono::Duration::seconds(sqs_queue.delay_seconds);
    let expires_at = now + chrono::Duration::seconds(sqs_queue.message_retention_period_seconds);

    let sqs_message = SqsMessage {
        message_id: uuid::Uuid::new_v4().to_string(),
        queue_id: sqs_queue.id,
        body,
        message_attributes: None,
        aws_trace_header: None,
        sent_at: now,
        visible_at,
        expires_at,
        receive_count: 0,
        receipt_handle: None,
        first_received_at: None,
        message_group_id: None,
        message_deduplication_id: None,
    };

    state.sqs_store.send_message(&sqs_message).await?;
    Ok(())
}

fn parse_sqs_endpoint_arn(endpoint: &str) -> Option<SqsQueueId> {
    let parts: Vec<&str> = endpoint.split(':').collect();
    if parts.len() == 6 && parts[0] == "arn" && parts[1] == "aws" && parts[2] == "sqs" {
        return Some(SqsQueueId {
            region: parts[3].to_string(),
            account_id: parts[4].to_string(),
            name: parts[5].to_string(),
        });
    }
    None
}

struct SqsQueueId {
    region: String,
    account_id: String,
    name: String,
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

fn create_topic_fields(params: &TopicListParams) -> CreateTopicFields {
    let error = params.create_error.clone().unwrap_or_default();

    CreateTopicFields {
        has_error: !error.is_empty(),
        error,
        topic_name: params.create_topic_name.clone().unwrap_or_default(),
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
        display_name: params.create_display_name.clone().unwrap_or_default(),
    }
}

fn subscription_form_fields(params: &TopicDetailParams) -> SubscriptionFormFields {
    let error = params.subscription_error.clone().unwrap_or_default();

    SubscriptionFormFields {
        has_error: !error.is_empty(),
        error,
        protocol: params.subscription_protocol.clone().unwrap_or_default(),
        endpoint: params.subscription_endpoint.clone().unwrap_or_default(),
        raw_message_delivery: params.subscription_raw_message_delivery.as_deref() == Some("on"),
    }
}

fn publish_form_fields(params: &TopicDetailParams) -> PublishFormFields {
    let error = params.publish_error.clone().unwrap_or_default();

    PublishFormFields {
        has_error: !error.is_empty(),
        error,
        message: params.publish_message.clone().unwrap_or_default(),
        subject: params.publish_subject.clone().unwrap_or_default(),
    }
}

fn scoped_page_path(base_path: &str, params: &TopicListParams) -> String {
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

fn create_topic_error_redirect(form: &CreateTopicForm, message: &str) -> Redirect {
    let return_to = safe_return_to(form.return_to.as_deref(), "/sns/topics");
    Redirect::to(&append_query_params(
        return_to,
        &[
            ("create_error", message),
            ("create_topic_name", form.name.trim()),
            ("create_region", form.region.trim()),
            ("create_account_id", form.account_id.trim()),
            ("create_display_name", form.display_name.trim()),
        ],
    ))
}

fn subscription_error_redirect(
    topic_id: i64,
    protocol: &str,
    endpoint: &str,
    raw_message_delivery: bool,
    message: &str,
) -> Redirect {
    Redirect::to(&append_query_params(
        topic_detail_path(topic_id),
        &[
            ("subscription_error", message),
            ("subscription_protocol", protocol),
            ("subscription_endpoint", endpoint),
            (
                "subscription_raw_message_delivery",
                if raw_message_delivery { "on" } else { "" },
            ),
        ],
    ))
}

fn publish_error_redirect(
    topic_id: i64,
    message: &str,
    subject: &str,
    error: &str,
) -> Redirect {
    Redirect::to(&append_query_params(
        topic_detail_path(topic_id),
        &[
            ("publish_error", error),
            ("publish_message", message),
            ("publish_subject", subject),
        ],
    ))
}

fn topic_detail_path(topic_id: i64) -> String {
    format!("/sns/topics/{topic_id}")
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

    let valid_sns_path =
        return_to == "/sns" || return_to.starts_with("/sns/") || return_to.starts_with("/sns?");
    if valid_sns_path && !return_to.starts_with("//") && !return_to.contains(['\n', '\r']) {
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
        title: "SNS".to_string(),
        description: "Review topics and subscriptions for a specific account and region. Use this page to monitor resources and confirm recent changes.".to_string(),
        actions: vec![
            HeaderAction::link(
                "Browse topics",
                format!("/sns/topics?region={region}&account_id={account_id}"),
                "btn btn-primary",
            ),
            HeaderAction::link(
                "JSON API",
                format!("/sns/api/topics?region={region}&account_id={account_id}"),
                "btn btn-outline",
            ),
        ],
    }
    .render()
}

fn dashboard_stats_html(
    region: &str,
    account_id: &str,
    total_topics: usize,
    total_subscriptions: usize,
) -> Result<String, askama::Error> {
    Ok(format!(
        r##"<div id="dashboard-stats" class="mb-5">{}</div>"##,
        dashboard_stats_cards_html(region, account_id, total_topics, total_subscriptions)?
    ))
}

fn dashboard_stats_cards_html(
    region: &str,
    account_id: &str,
    total_topics: usize,
    total_subscriptions: usize,
) -> Result<String, askama::Error> {
    StatBlockGrid {
        grid_class: "grid gap-3 md:grid-cols-3",
        blocks: vec![
            StatBlock {
                title: "Topics".to_string(),
                value: total_topics.to_string(),
                value_class: "text-primary",
                description: format!("{account_id} / {region}"),
            },
            StatBlock {
                title: "Subscriptions".to_string(),
                value: total_subscriptions.to_string(),
                value_class: "text-secondary",
                description: "active subscriptions".to_string(),
            },
            StatBlock {
                title: "Supported".to_string(),
                value: "sqs".to_string(),
                value_class: "text-accent",
                description: "delivery protocol".to_string(),
            },
        ],
    }
    .render()
}

fn topic_list_empty_state_html() -> Result<String, askama::Error> {
    EmptyState {
        title: "No topics found".to_string(),
        message: "Create a topic or adjust the account, region, or prefix filter.".to_string(),
    }
    .render()
}

fn topic_action_card_html(topic: &SnsTopic) -> Result<String, askama::Error> {
    ActionCard {
        title: "Topic actions".to_string(),
        grid_class: "grid-cols-2",
        actions: vec![
            ActionCardAction::link(
                "Back",
                format!(
                    "/sns/topics?region={}&account_id={}",
                    topic.region, topic.account_id
                ),
                "btn btn-ghost",
            ),
            ActionCardAction::link(
                "Reload",
                format!("/sns/topics/{}", topic.id),
                "btn btn-outline",
            ),
            ActionCardAction::post(
                "Delete",
                format!("/sns/topics/{}/delete", topic.id),
                "btn btn-error col-span-2 w-full",
                Some(
                    "return hiraethConfirmSubmit(this, 'Delete this topic and all subscriptions?', 'Deleting...')"
                        .to_string(),
                ),
            ),
        ],
    }
    .render()
}

fn topic_metadata_list_html(topic: &SnsTopic) -> Result<String, askama::Error> {
    let mut entries = vec![
        MetadataEntry {
            label: "Database ID".to_string(),
            value: topic.id.to_string(),
            value_class: "font-mono",
        },
        MetadataEntry {
            label: "Created".to_string(),
            value: format_datetime(topic.created_at),
            value_class: "font-mono",
        },
    ];
    if let Some(ref value) = topic.delivery_policy {
        entries.push(MetadataEntry {
            label: "DeliveryPolicy".to_string(),
            value: value.clone(),
            value_class: "font-mono",
        });
    }
    if let Some(ref value) = topic.fifo_topic {
        entries.push(MetadataEntry {
            label: "FifoTopic".to_string(),
            value: value.clone(),
            value_class: "font-mono",
        });
    }
    if let Some(ref value) = topic.signature_version {
        entries.push(MetadataEntry {
            label: "SignatureVersion".to_string(),
            value: value.clone(),
            value_class: "font-mono",
        });
    }
    if let Some(ref value) = topic.tracing_config {
        entries.push(MetadataEntry {
            label: "TracingConfig".to_string(),
            value: value.clone(),
            value_class: "font-mono",
        });
    }
    if let Some(ref value) = topic.kms_master_key_id {
        entries.push(MetadataEntry {
            label: "KmsMasterKeyId".to_string(),
            value: value.clone(),
            value_class: "font-mono",
        });
    }
    if let Some(ref value) = topic.data_protection_policy {
        entries.push(MetadataEntry {
            label: "DataProtectionPolicy".to_string(),
            value: value.clone(),
            value_class: "font-mono",
        });
    }
    MetadataList { entries }.render()
}

async fn load_topic_summaries(
    state: &WebState,
    params: &TopicListParams,
) -> Result<Vec<TopicSummary>, WebError> {
    let topics = state
        .sns_store
        .list_topics(
            &params.region,
            &params.account_id,
            params.prefix.as_deref().filter(|prefix| !prefix.is_empty()),
            Some(100),
        )
        .await?;

    let mut summaries = Vec::with_capacity(topics.len());
    for topic in topics {
        summaries.push(topic_summary(state, topic).await?);
    }

    Ok(summaries)
}

async fn load_topic_by_id(state: &WebState, topic_id: i64) -> Result<SnsTopic, WebError> {
    state
        .sns_store
        .get_topic_by_id(topic_id)
        .await?
        .ok_or_else(|| StoreError::NotFound("topic not found".to_string()).into())
}

async fn topic_summary(state: &WebState, topic: SnsTopic) -> Result<TopicSummary, WebError> {
    let arn = topic_arn(&topic.name, &topic.region, &topic.account_id);
    let subscriptions = state.sns_store.list_subscriptions_by_topic(&arn).await?;

    Ok(TopicSummary {
        id: topic.id,
        name: topic.name,
        region: topic.region,
        account_id: topic.account_id,
        display_name: topic.display_name,
        subscription_count: subscriptions.len(),
    })
}

fn topic_arn(name: &str, region: &str, account_id: &str) -> String {
    format!("arn:aws:sns:{region}:{account_id}:{name}")
}

fn subscription_summary(subscription: SnsSubscription) -> SubscriptionSummary {
    SubscriptionSummary {
        id: subscription.id,
        protocol: subscription.protocol,
        endpoint: subscription.endpoint,
        subscription_arn: subscription.subscription_arn,
        raw_message_delivery: subscription.raw_message_delivery.as_deref() == Some("true"),
        created_at: format_datetime(subscription.created_at),
    }
}

fn validate_required(field_name: &str, value: &str) -> Result<(), WebError> {
    if value.is_empty() {
        return Err(WebError::bad_request(format!("{field_name} is required")));
    }

    Ok(())
}

fn validate_topic_name(topic_name: &str) -> Result<(), WebError> {
    if topic_name.len() > 256 {
        return Err(WebError::bad_request(
            "Topic name must be at most 256 characters",
        ));
    }

    let valid_chars = topic_name
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || ch == '-' || ch == '_');
    if !valid_chars {
        return Err(WebError::bad_request(
            "Topic name may only contain alphanumeric characters, hyphens, and underscores",
        ));
    }

    Ok(())
}

fn format_datetime(value: chrono::NaiveDateTime) -> String {
    value.format("%Y-%m-%d %H:%M:%S UTC").to_string()
}

#[cfg(test)]
mod tests {
    use super::{
        append_query_params, feedback_from_params, safe_return_to, topic_arn, validate_topic_name,
    };

    #[test]
    fn topic_arn_uses_identity_fields() {
        let arn = topic_arn("orders", "us-east-1", "000000000000");
        assert_eq!(arn, "arn:aws:sns:us-east-1:000000000000:orders");
    }

    #[test]
    fn validate_topic_name_rejects_invalid_characters() {
        let result = validate_topic_name("orders<billing>");
        assert!(result.is_err());
    }

    #[test]
    fn validate_topic_name_accepts_valid_name() {
        let result = validate_topic_name("orders-billing_123");
        assert!(result.is_ok());
    }

    #[test]
    fn append_query_params_encodes_values_and_preserves_existing_query() {
        let path = append_query_params(
            "/sns/topics?region=us-east-1".to_string(),
            &[
                ("feedback", "Created topic orders & billing."),
                ("empty", ""),
            ],
        );

        assert_eq!(
            path,
            "/sns/topics?region=us-east-1&feedback=Created%20topic%20orders%20%26%20billing."
        );
    }

    #[test]
    fn safe_return_to_rejects_external_paths() {
        assert_eq!(
            safe_return_to(Some("https://example.com"), "/sns/topics"),
            "/sns/topics"
        );
        assert_eq!(
            safe_return_to(Some("/sns.evil"), "/sns/topics"),
            "/sns/topics"
        );
        assert_eq!(
            safe_return_to(Some("/sns?region=test"), "/sns/topics"),
            "/sns?region=test"
        );
    }

    #[test]
    fn feedback_from_params_defaults_to_success_class() {
        let feedback = feedback_from_params(&None, &Some("Created topic.".to_string()));

        assert!(feedback.has_message);
        assert_eq!(feedback.alert_class, "alert-success");
        assert_eq!(feedback.message, "Created topic.");
    }
}
