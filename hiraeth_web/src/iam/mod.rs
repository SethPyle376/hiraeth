use std::collections::BTreeSet;

use askama::Template;
use axum::{
    Router,
    extract::{Form, Path, Query, State},
    response::{Html, Redirect},
    routing::{get, post},
};
use hiraeth_store::{
    StoreError,
    iam::{AccessKeyStore, NewPrincipal, PrincipalInlinePolicyStore, PrincipalStore},
};
use serde::Deserialize;
use uuid::Uuid;

use crate::{
    WebState,
    components::{
        ActionCard, ActionCardAction, EmptyState, HeaderAction, MetadataEntry, MetadataList,
        PageHeader, StatBlock, StatBlockGrid, SummaryCard, SummaryCardGrid,
    },
    error::WebError,
    templates::{IamDashboardTemplate, IamPrincipalDetailTemplate},
};

pub(crate) struct IamDashboardStats {
    pub(crate) principal_count: usize,
    pub(crate) access_key_count: usize,
    pub(crate) inline_policy_count: usize,
    pub(crate) account_count: usize,
}

#[derive(Debug, Clone, Deserialize)]
struct IamDashboardParams {
    #[serde(default)]
    feedback: Option<String>,
    #[serde(default)]
    feedback_kind: Option<String>,
    #[serde(default)]
    create_error: Option<String>,
    #[serde(default)]
    create_account_id: Option<String>,
    #[serde(default)]
    create_kind: Option<String>,
    #[serde(default)]
    create_name: Option<String>,
    #[serde(default)]
    create_path: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct IamPrincipalDetailParams {
    #[serde(default)]
    feedback: Option<String>,
    #[serde(default)]
    feedback_kind: Option<String>,
    #[serde(default)]
    access_key_error: Option<String>,
    #[serde(default)]
    policy_error: Option<String>,
    #[serde(default)]
    policy_name: Option<String>,
    #[serde(default)]
    policy_document: Option<String>,
    #[serde(default)]
    policy_open: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct CreatePrincipalForm {
    account_id: String,
    kind: String,
    name: String,
    path: String,
}

#[derive(Debug, Clone, Deserialize)]
struct AccessKeyForm {
    #[serde(default)]
    key_id: String,
    #[serde(default)]
    secret_key: String,
}

#[derive(Debug, Clone, Deserialize)]
struct DeleteAccessKeyForm {
    key_id: String,
}

#[derive(Debug, Clone, Deserialize)]
struct InlinePolicyForm {
    policy_name: String,
    #[serde(default)]
    policy_document: String,
}

pub(crate) struct IamPrincipalSummary {
    pub(crate) id: i64,
    pub(crate) account_id: String,
    pub(crate) kind: String,
    pub(crate) name: String,
    pub(crate) created_at: String,
    pub(crate) access_key_count: usize,
    pub(crate) inline_policy_count: usize,
}

pub(crate) struct IamPrincipalDetailView {
    pub(crate) id: i64,
    pub(crate) account_id: String,
    pub(crate) kind: String,
    pub(crate) name: String,
    pub(crate) created_at: String,
    pub(crate) access_key_count: usize,
    pub(crate) inline_policy_count: usize,
}

pub(crate) struct IamPrincipalAccessKeyView {
    pub(crate) key_id: String,
    pub(crate) secret_key: String,
    pub(crate) masked_secret_key: String,
    pub(crate) created_at: String,
}

pub(crate) struct IamPrincipalInlinePolicyView {
    pub(crate) policy_name: String,
    pub(crate) created_at: String,
    pub(crate) updated_at: String,
    pub(crate) pretty_document: String,
}

struct PageFeedback {
    message: String,
    alert_class: &'static str,
    has_message: bool,
}

struct CreatePrincipalFields {
    error: String,
    has_error: bool,
    account_id: String,
    kind: String,
    name: String,
    path: String,
}

struct InlinePolicyFields {
    error: String,
    has_error: bool,
    policy_name: String,
    policy_document: String,
    open_panel: bool,
}

pub fn router() -> Router<WebState> {
    Router::new()
        .route("/", get(dashboard))
        .route("/principals", get(dashboard))
        .route("/principals/create", post(create_principal))
        .route("/principals/{principal_id}", get(principal_detail))
        .route("/principals/{principal_id}/delete", post(delete_principal))
        .route(
            "/principals/{principal_id}/access-keys",
            post(create_access_key),
        )
        .route(
            "/principals/{principal_id}/access-keys/delete",
            post(delete_access_key),
        )
        .route(
            "/principals/{principal_id}/inline-policies",
            post(put_inline_policy),
        )
        .route(
            "/principals/{principal_id}/inline-policies/delete",
            post(delete_inline_policy),
        )
}

async fn dashboard(
    State(state): State<WebState>,
    Query(params): Query<IamDashboardParams>,
) -> Result<Html<String>, WebError> {
    let principals = load_principal_summaries(&state).await?;
    let stats = dashboard_stats(&principals);
    let page_header_html = dashboard_page_header()?;
    let stats_html = dashboard_stats_html(&stats)?;
    let empty_state_html = principal_empty_state_html()?;
    let feedback = feedback_from_params(&params.feedback_kind, &params.feedback);
    let create_fields = create_principal_fields(&params);
    let template = IamDashboardTemplate {
        page_header_html: &page_header_html,
        stats_html: &stats_html,
        empty_state_html: &empty_state_html,
        feedback_message: &feedback.message,
        feedback_class: feedback.alert_class,
        has_feedback: feedback.has_message,
        create_error: &create_fields.error,
        has_create_error: create_fields.has_error,
        create_account_id: &create_fields.account_id,
        create_kind: &create_fields.kind,
        create_name: &create_fields.name,
        create_path: &create_fields.path,
        principal_count: principals.len(),
        principals: &principals,
        has_principals: !principals.is_empty(),
    };

    Ok(Html(template.render()?))
}

async fn principal_detail(
    State(state): State<WebState>,
    Path(principal_id): Path<i64>,
    Query(params): Query<IamPrincipalDetailParams>,
) -> Result<Html<String>, WebError> {
    let principal = state
        .iam_store
        .get_principal(principal_id)
        .await?
        .ok_or_else(|| StoreError::NotFound(format!("Principal not found: {principal_id}")))?;
    let access_keys = state
        .iam_store
        .list_access_keys_for_principal(principal_id)
        .await?;
    let inline_policies = state
        .iam_store
        .get_inline_policies_for_principal(principal_id)
        .await?;

    let access_key_views = access_keys
        .into_iter()
        .map(|access_key| IamPrincipalAccessKeyView {
            key_id: access_key.key_id,
            masked_secret_key: mask_secret_key(&access_key.secret_key),
            secret_key: access_key.secret_key,
            created_at: format_timestamp(access_key.created_at),
        })
        .collect::<Vec<_>>();
    let mut inline_policy_views = inline_policies
        .into_iter()
        .map(|policy| IamPrincipalInlinePolicyView {
            policy_name: policy.policy_name,
            created_at: format_timestamp(policy.created_at),
            updated_at: format_timestamp(policy.updated_at),
            pretty_document: prettify_json(&policy.policy_document),
        })
        .collect::<Vec<_>>();
    inline_policy_views.sort_by(|left, right| left.policy_name.cmp(&right.policy_name));

    let principal_view = IamPrincipalDetailView {
        id: principal.id,
        account_id: principal.account_id,
        kind: principal.kind,
        name: principal.name,
        created_at: format_timestamp(principal.created_at),
        access_key_count: access_key_views.len(),
        inline_policy_count: inline_policy_views.len(),
    };
    let action_card_html = principal_action_card_html(principal_id)?;
    let summary_cards_html = principal_summary_cards_html(&principal_view)?;
    let metadata_list_html = principal_metadata_list_html(&principal_view)?;
    let feedback = feedback_from_params(&params.feedback_kind, &params.feedback);
    let policy_fields = inline_policy_fields(&params);
    let access_key_error = params.access_key_error.clone().unwrap_or_default();

    let template = IamPrincipalDetailTemplate {
        action_card_html: &action_card_html,
        summary_cards_html: &summary_cards_html,
        metadata_list_html: &metadata_list_html,
        feedback_message: &feedback.message,
        feedback_class: feedback.alert_class,
        has_feedback: feedback.has_message,
        access_key_error: &access_key_error,
        has_access_key_error: !access_key_error.is_empty(),
        policy_error: &policy_fields.error,
        has_policy_error: policy_fields.has_error,
        policy_name: &policy_fields.policy_name,
        policy_document: &policy_fields.policy_document,
        policy_panel_open: policy_fields.open_panel,
        principal: &principal_view,
        access_keys: &access_key_views,
        inline_policies: &inline_policy_views,
        has_access_keys: !access_key_views.is_empty(),
        has_inline_policies: !inline_policy_views.is_empty(),
    };

    Ok(Html(template.render()?))
}

async fn create_principal(
    State(state): State<WebState>,
    Form(form): Form<CreatePrincipalForm>,
) -> Result<Redirect, WebError> {
    let account_id = form.account_id.trim();
    let kind = form.kind.trim();
    let name = form.name.trim();
    let path = normalize_path(&form.path);

    if let Err(error) = validate_required("Account ID", account_id)
        .and_then(|_| validate_required("Principal name", name))
        .and_then(|_| validate_principal_kind(kind))
    {
        return Ok(create_principal_error_redirect(&form, error.message()));
    }

    match state
        .iam_store
        .create_principal(NewPrincipal {
            account_id: account_id.to_string(),
            kind: kind.to_string(),
            name: name.to_string(),
            path,
            user_id: new_principal_id(),
        })
        .await
    {
        Ok(principal) => Ok(feedback_redirect(
            format!("/iam/principals/{}", principal.id),
            "success",
            &format!("Created principal {name}."),
        )),
        Err(StoreError::Conflict(message)) => Ok(create_principal_error_redirect(&form, &message)),
        Err(error) => Err(error.into()),
    }
}

async fn delete_principal(
    State(state): State<WebState>,
    Path(principal_id): Path<i64>,
) -> Result<Redirect, WebError> {
    let principal = state
        .iam_store
        .get_principal(principal_id)
        .await?
        .ok_or_else(|| StoreError::NotFound(format!("Principal not found: {principal_id}")))?;
    state.iam_store.delete_principal(principal_id).await?;

    Ok(feedback_redirect(
        "/iam".to_string(),
        "success",
        &format!("Deleted principal {}.", principal.name),
    ))
}

async fn create_access_key(
    State(state): State<WebState>,
    Path(principal_id): Path<i64>,
    Form(form): Form<AccessKeyForm>,
) -> Result<Redirect, WebError> {
    load_principal(&state, principal_id).await?;
    let key_id = form
        .key_id
        .trim()
        .to_string()
        .if_empty_else(new_access_key_id);
    let secret_key = form
        .secret_key
        .trim()
        .to_string()
        .if_empty_else(new_secret_access_key);

    if let Err(error) = validate_required("Access key id", &key_id)
        .and_then(|_| validate_required("Secret key", &secret_key))
    {
        return Ok(access_key_error_redirect(principal_id, error.message()));
    }

    match state
        .iam_store
        .insert_secret_key(&key_id, &secret_key, principal_id)
        .await
    {
        Ok(_) => Ok(feedback_redirect(
            principal_detail_path(principal_id),
            "success",
            &format!("Created access key {key_id}."),
        )),
        Err(StoreError::Conflict(message)) => Ok(access_key_error_redirect(principal_id, &message)),
        Err(error) => Err(error.into()),
    }
}

async fn delete_access_key(
    State(state): State<WebState>,
    Path(principal_id): Path<i64>,
    Form(form): Form<DeleteAccessKeyForm>,
) -> Result<Redirect, WebError> {
    load_principal(&state, principal_id).await?;
    let key_id = form.key_id.trim();
    if let Err(error) = validate_required("Access key id", key_id) {
        return Ok(access_key_error_redirect(principal_id, error.message()));
    }

    state
        .iam_store
        .delete_access_key_for_principal(principal_id, key_id)
        .await?;

    Ok(feedback_redirect(
        principal_detail_path(principal_id),
        "success",
        &format!("Deleted access key {key_id}."),
    ))
}

async fn put_inline_policy(
    State(state): State<WebState>,
    Path(principal_id): Path<i64>,
    Form(form): Form<InlinePolicyForm>,
) -> Result<Redirect, WebError> {
    load_principal(&state, principal_id).await?;
    let policy_name = form.policy_name.trim();
    if let Err(error) = validate_required("Policy name", policy_name)
        .and_then(|_| validate_required("Policy document", &form.policy_document))
    {
        return Ok(inline_policy_error_redirect(
            principal_id,
            &form,
            error.message(),
        ));
    }

    let policy_document = match normalize_policy_document(&form.policy_document) {
        Ok(policy_document) => policy_document,
        Err(error) => {
            return Ok(inline_policy_error_redirect(
                principal_id,
                &form,
                error.message(),
            ));
        }
    };

    state
        .iam_store
        .put_inline_policy(principal_id, policy_name, &policy_document)
        .await?;

    Ok(feedback_redirect(
        principal_policy_path(principal_id),
        "success",
        &format!("Saved inline policy {policy_name}."),
    ))
}

async fn delete_inline_policy(
    State(state): State<WebState>,
    Path(principal_id): Path<i64>,
    Form(form): Form<InlinePolicyForm>,
) -> Result<Redirect, WebError> {
    load_principal(&state, principal_id).await?;
    let policy_name = form.policy_name.trim();
    if let Err(error) = validate_required("Policy name", policy_name) {
        return Ok(inline_policy_error_redirect(
            principal_id,
            &form,
            error.message(),
        ));
    }

    state
        .iam_store
        .delete_inline_policy(principal_id, policy_name)
        .await?;

    Ok(feedback_redirect(
        principal_policy_path(principal_id),
        "success",
        &format!("Deleted inline policy {policy_name}."),
    ))
}

async fn load_principal_summaries(state: &WebState) -> Result<Vec<IamPrincipalSummary>, WebError> {
    let principals = state.iam_store.list_principals().await?;
    let mut summaries = Vec::with_capacity(principals.len());

    for principal in principals {
        let access_keys = state
            .iam_store
            .list_access_keys_for_principal(principal.id)
            .await?;
        let inline_policies = state
            .iam_store
            .get_inline_policies_for_principal(principal.id)
            .await?;

        summaries.push(IamPrincipalSummary {
            id: principal.id,
            account_id: principal.account_id,
            kind: principal.kind,
            name: principal.name,
            created_at: format_timestamp(principal.created_at),
            access_key_count: access_keys.len(),
            inline_policy_count: inline_policies.len(),
        });
    }

    Ok(summaries)
}

fn dashboard_stats(principals: &[IamPrincipalSummary]) -> IamDashboardStats {
    IamDashboardStats {
        principal_count: principals.len(),
        access_key_count: principals
            .iter()
            .map(|principal| principal.access_key_count)
            .sum(),
        inline_policy_count: principals
            .iter()
            .map(|principal| principal.inline_policy_count)
            .sum(),
        account_count: principals
            .iter()
            .map(|principal| principal.account_id.as_str())
            .collect::<BTreeSet<_>>()
            .len(),
    }
}

async fn load_principal(
    state: &WebState,
    principal_id: i64,
) -> Result<hiraeth_store::iam::Principal, WebError> {
    state
        .iam_store
        .get_principal(principal_id)
        .await?
        .ok_or_else(|| StoreError::NotFound(format!("Principal not found: {principal_id}")).into())
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

fn create_principal_fields(params: &IamDashboardParams) -> CreatePrincipalFields {
    let error = params.create_error.clone().unwrap_or_default();
    let kind = params
        .create_kind
        .as_deref()
        .filter(|value| matches!(*value, "user" | "role"))
        .unwrap_or("user")
        .to_string();

    CreatePrincipalFields {
        has_error: !error.is_empty(),
        error,
        account_id: params
            .create_account_id
            .clone()
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| "000000000000".to_string()),
        kind,
        name: params.create_name.clone().unwrap_or_default(),
        path: params
            .create_path
            .clone()
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| "/".to_string()),
    }
}

fn inline_policy_fields(params: &IamPrincipalDetailParams) -> InlinePolicyFields {
    let error = params.policy_error.clone().unwrap_or_default();
    let open_panel = !error.is_empty() || params.policy_open.as_deref() == Some("1");

    InlinePolicyFields {
        has_error: !error.is_empty(),
        error,
        policy_name: params.policy_name.clone().unwrap_or_default(),
        policy_document: params
            .policy_document
            .clone()
            .unwrap_or_else(default_policy_document),
        open_panel,
    }
}

fn create_principal_error_redirect(form: &CreatePrincipalForm, message: &str) -> Redirect {
    Redirect::to(&append_query_params(
        "/iam".to_string(),
        &[
            ("create_error", message),
            ("create_account_id", form.account_id.trim()),
            ("create_kind", form.kind.trim()),
            ("create_name", form.name.trim()),
            ("create_path", form.path.trim()),
        ],
    ))
}

fn access_key_error_redirect(principal_id: i64, message: &str) -> Redirect {
    Redirect::to(&append_query_params(
        principal_detail_path(principal_id),
        &[("access_key_error", message)],
    ))
}

fn inline_policy_error_redirect(
    principal_id: i64,
    form: &InlinePolicyForm,
    message: &str,
) -> Redirect {
    Redirect::to(&append_query_params(
        principal_policy_path(principal_id),
        &[
            ("policy_error", message),
            ("policy_name", form.policy_name.trim()),
            ("policy_document", form.policy_document.as_str()),
        ],
    ))
}

fn principal_detail_path(principal_id: i64) -> String {
    format!("/iam/principals/{principal_id}")
}

fn principal_policy_path(principal_id: i64) -> String {
    append_query_params(principal_detail_path(principal_id), &[("policy_open", "1")])
}

fn feedback_redirect(path: String, kind: &str, message: &str) -> Redirect {
    Redirect::to(&append_query_params(
        path,
        &[("feedback_kind", kind), ("feedback", message)],
    ))
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

fn validate_required(field_name: &str, value: &str) -> Result<(), WebError> {
    if value.trim().is_empty() {
        return Err(WebError::bad_request(format!(
            "{field_name} must not be empty"
        )));
    }

    Ok(())
}

fn validate_principal_kind(kind: &str) -> Result<(), WebError> {
    if !matches!(kind, "user" | "role") {
        return Err(WebError::bad_request(
            "Principal kind must be either user or role",
        ));
    }

    Ok(())
}

fn normalize_path(path: &str) -> String {
    let trimmed = path.trim();
    if trimmed.is_empty() || trimmed == "/" {
        "/".to_string()
    } else {
        format!("/{}/", trimmed.trim_matches('/'))
    }
}

fn normalize_policy_document(document: &str) -> Result<String, WebError> {
    let value = serde_json::from_str::<serde_json::Value>(document).map_err(|error| {
        WebError::bad_request(format!("Policy document is not valid JSON: {error}"))
    })?;
    serde_json::to_string(&value).map_err(|error| {
        WebError::internal(format!("Policy document serialization failed: {error}"))
    })
}

fn default_policy_document() -> String {
    serde_json::to_string_pretty(&serde_json::json!({
        "Version": "2012-10-17",
        "Statement": [{
            "Effect": "Allow",
            "Action": "*",
            "Resource": "*"
        }]
    }))
    .expect("default policy document should serialize")
}

fn new_principal_id() -> String {
    format!("AIDA{}", Uuid::new_v4().simple().to_string().to_uppercase())
}

fn new_access_key_id() -> String {
    let random = Uuid::new_v4().simple().to_string().to_uppercase();
    format!("AKIA{}", &random[..16])
}

fn new_secret_access_key() -> String {
    let random = format!("{}{}", Uuid::new_v4().simple(), Uuid::new_v4().simple());
    random[..40].to_string()
}

trait EmptyStringExt {
    fn if_empty_else(self, fallback: impl FnOnce() -> String) -> String;
}

impl EmptyStringExt for String {
    fn if_empty_else(self, fallback: impl FnOnce() -> String) -> String {
        if self.is_empty() { fallback() } else { self }
    }
}

fn dashboard_page_header() -> Result<String, askama::Error> {
    PageHeader {
        eyebrow: "Service Dashboard".to_string(),
        title: "IAM".to_string(),
        description: "Review principals, access keys, and inline identity policies for the current environment.".to_string(),
        actions: vec![HeaderAction::link(
            "Add principal",
            "#add-principal",
            "btn btn-primary",
        )],
    }
    .render()
}

fn dashboard_stats_html(stats: &IamDashboardStats) -> Result<String, askama::Error> {
    StatBlockGrid {
        grid_class: "grid gap-3 md:grid-cols-4",
        blocks: vec![
            StatBlock {
                title: "Principals".to_string(),
                value: stats.principal_count.to_string(),
                value_class: "text-primary",
                description: "configured identities".to_string(),
            },
            StatBlock {
                title: "Access keys".to_string(),
                value: stats.access_key_count.to_string(),
                value_class: "text-secondary",
                description: "credentials assigned to principals".to_string(),
            },
            StatBlock {
                title: "Inline policies".to_string(),
                value: stats.inline_policy_count.to_string(),
                value_class: "text-accent",
                description: "attached policy documents".to_string(),
            },
            StatBlock {
                title: "Accounts".to_string(),
                value: stats.account_count.to_string(),
                value_class: "",
                description: "accounts represented".to_string(),
            },
        ],
    }
    .render()
}

fn principal_summary_cards_html(
    principal: &IamPrincipalDetailView,
) -> Result<String, askama::Error> {
    SummaryCardGrid {
        grid_class: "mb-6 grid gap-3 md:grid-cols-3",
        cards: vec![
            SummaryCard {
                title: "Access keys".to_string(),
                value: principal.access_key_count.to_string(),
                value_class: "text-secondary",
                description: "credentials assigned to this principal".to_string(),
            },
            SummaryCard {
                title: "Inline policies".to_string(),
                value: principal.inline_policy_count.to_string(),
                value_class: "text-accent",
                description: "policy documents".to_string(),
            },
            SummaryCard {
                title: "Account scope".to_string(),
                value: principal.account_id.clone(),
                value_class: "text-primary text-xl font-mono",
                description: format!("{} principal", principal.kind),
            },
        ],
    }
    .render()
}

fn principal_action_card_html(principal_id: i64) -> Result<String, askama::Error> {
    ActionCard {
        title: "Principal actions".to_string(),
        grid_class: "grid-cols-2",
        actions: vec![
            ActionCardAction::link("Back", "/iam", "btn btn-ghost"),
            ActionCardAction::link(
                "Reload",
                format!("/iam/principals/{principal_id}"),
                "btn btn-outline",
            ),
        ],
    }
    .render()
}

fn principal_metadata_list_html(
    principal: &IamPrincipalDetailView,
) -> Result<String, askama::Error> {
    MetadataList {
        entries: vec![
            MetadataEntry {
                label: "Name".to_string(),
                value: principal.name.clone(),
                value_class: "font-mono",
            },
            MetadataEntry {
                label: "Account ID".to_string(),
                value: principal.account_id.clone(),
                value_class: "font-mono",
            },
            MetadataEntry {
                label: "Kind".to_string(),
                value: principal.kind.clone(),
                value_class: "font-mono",
            },
            MetadataEntry {
                label: "Database ID".to_string(),
                value: principal.id.to_string(),
                value_class: "font-mono",
            },
            MetadataEntry {
                label: "Created".to_string(),
                value: principal.created_at.clone(),
                value_class: "font-mono",
            },
        ],
    }
    .render()
}

fn principal_empty_state_html() -> Result<String, askama::Error> {
    EmptyState {
        title: "No principals found".to_string(),
        message: "Once IAM identities are seeded or created, they will show up here.".to_string(),
    }
    .render()
}

fn format_timestamp(value: chrono::NaiveDateTime) -> String {
    value.format("%Y-%m-%d %H:%M:%S UTC").to_string()
}

fn prettify_json(document: &str) -> String {
    match serde_json::from_str::<serde_json::Value>(document) {
        Ok(value) => serde_json::to_string_pretty(&value).unwrap_or_else(|_| document.to_string()),
        Err(_) => document.to_string(),
    }
}

fn mask_secret_key(secret_key: &str) -> String {
    let chars = secret_key.chars().collect::<Vec<_>>();
    if chars.len() <= 8 {
        return "*".repeat(chars.len().max(6));
    }

    let prefix = chars.iter().take(4).collect::<String>();
    let suffix = chars.iter().skip(chars.len() - 4).collect::<String>();
    let stars = "*".repeat((chars.len() - 8).max(8));

    format!("{prefix}{stars}{suffix}")
}

#[cfg(test)]
mod tests {
    use super::{
        append_query_params, mask_secret_key, normalize_path, normalize_policy_document,
        validate_principal_kind,
    };

    #[test]
    fn mask_secret_key_preserves_prefix_and_suffix_for_long_values() {
        let masked = mask_secret_key("example-secret-value");

        assert!(masked.starts_with("exam"));
        assert!(masked.ends_with("alue"));
        assert_eq!(masked.len(), "example-secret-value".len());
    }

    #[test]
    fn mask_secret_key_fully_masks_short_values() {
        assert_eq!(mask_secret_key("secret"), "******");
    }

    #[test]
    fn normalize_path_adds_leading_and_trailing_slashes() {
        assert_eq!(normalize_path("team/dev"), "/team/dev/");
        assert_eq!(normalize_path("/team/dev/"), "/team/dev/");
        assert_eq!(normalize_path("  /  "), "/");
    }

    #[test]
    fn normalize_policy_document_minifies_valid_json() {
        let document = normalize_policy_document(
            r#"{
              "Version": "2012-10-17",
              "Statement": [{"Effect": "Allow", "Action": "*", "Resource": "*"}]
            }"#,
        )
        .expect("policy document should normalize");

        assert_eq!(
            document,
            r#"{"Statement":[{"Action":"*","Effect":"Allow","Resource":"*"}],"Version":"2012-10-17"}"#
        );
    }

    #[test]
    fn normalize_policy_document_rejects_invalid_json() {
        assert!(normalize_policy_document("{ nope").is_err());
    }

    #[test]
    fn append_query_params_encodes_values_and_preserves_existing_query() {
        let path = append_query_params(
            "/iam/principals/1?policy_open=1".to_string(),
            &[("policy_error", "bad json"), ("policy_name", "a/b")],
        );

        assert_eq!(
            path,
            "/iam/principals/1?policy_open=1&policy_error=bad%20json&policy_name=a%2Fb"
        );
    }

    #[test]
    fn validate_principal_kind_accepts_user_and_role() {
        assert!(validate_principal_kind("user").is_ok());
        assert!(validate_principal_kind("role").is_ok());
        assert!(validate_principal_kind("group").is_err());
    }
}
