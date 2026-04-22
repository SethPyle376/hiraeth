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

pub fn router() -> Router<WebState> {
    Router::new()
        .route("/", get(dashboard))
        .route("/principals", get(dashboard))
        .route("/principals/{principal_id}", get(principal_detail))
}

async fn dashboard(State(state): State<WebState>) -> Result<Html<String>, WebError> {
    let principals = load_principal_summaries(&state).await?;
    let stats = dashboard_stats(&principals);
    let page_header_html = dashboard_page_header()?;
    let stats_html = dashboard_stats_html(&stats)?;
    let empty_state_html = principal_empty_state_html()?;
    let template = IamDashboardTemplate {
        page_header_html: &page_header_html,
        stats_html: &stats_html,
        empty_state_html: &empty_state_html,
        principal_count: principals.len(),
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

    let template = IamPrincipalDetailTemplate {
        action_card_html: &action_card_html,
        summary_cards_html: &summary_cards_html,
        metadata_list_html: &metadata_list_html,
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

fn dashboard_page_header() -> Result<String, askama::Error> {
    PageHeader {
        eyebrow: "Service Dashboard".to_string(),
        title: "IAM".to_string(),
        description: "Review principals, access keys, and inline identity policies for the current environment.".to_string(),
        actions: vec![
            HeaderAction::link("Browse principals", "/iam/principals", "btn btn-primary"),
            HeaderAction::disabled("View mode", "btn btn-outline btn-disabled"),
        ],
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
        grid_class: "sm:grid-cols-2",
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
    use super::mask_secret_key;

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
}
