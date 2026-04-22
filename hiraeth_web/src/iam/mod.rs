use std::collections::BTreeSet;

use askama::Template;
use axum::{
    Router,
    extract::{Path, State},
    response::Html,
    routing::get,
};
use hiraeth_store::{
    StoreError,
    iam::{AccessKeyStore, PrincipalInlinePolicyStore, PrincipalStore},
};

use crate::{
    WebState,
    error::WebError,
    templates::{IamDashboardTemplate, IamPrincipalDetailTemplate},
};

pub(crate) struct IamDashboardStats {
    pub(crate) principal_count: usize,
    pub(crate) access_key_count: usize,
    pub(crate) inline_policy_count: usize,
    pub(crate) account_count: usize,
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
    pub(crate) created_at: String,
}

pub(crate) struct IamPrincipalInlinePolicyView {
    pub(crate) policy_name: String,
    pub(crate) created_at: String,
    pub(crate) updated_at: String,
    pub(crate) pretty_document: String,
}

pub fn router() -> Router<WebState> {
    Router::new()
        .route("/", get(dashboard))
        .route("/principals", get(dashboard))
        .route("/principals/{principal_id}", get(principal_detail))
}

async fn dashboard(State(state): State<WebState>) -> Result<Html<String>, WebError> {
    let principals = load_principal_summaries(&state).await?;
    let stats = dashboard_stats(&principals);
    let template = IamDashboardTemplate {
        stats: &stats,
        principals: &principals,
        has_principals: !principals.is_empty(),
    };

    Ok(Html(template.render()?))
}

async fn principal_detail(
    State(state): State<WebState>,
    Path(principal_id): Path<i64>,
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

    let template = IamPrincipalDetailTemplate {
        principal: &principal_view,
        access_keys: &access_key_views,
        inline_policies: &inline_policy_views,
        has_access_keys: !access_key_views.is_empty(),
        has_inline_policies: !inline_policy_views.is_empty(),
    };

    Ok(Html(template.render()?))
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

fn format_timestamp(value: chrono::NaiveDateTime) -> String {
    value.format("%Y-%m-%d %H:%M:%S UTC").to_string()
}

fn prettify_json(document: &str) -> String {
    match serde_json::from_str::<serde_json::Value>(document) {
        Ok(value) => serde_json::to_string_pretty(&value).unwrap_or_else(|_| document.to_string()),
        Err(_) => document.to_string(),
    }
}
