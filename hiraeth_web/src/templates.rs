use askama::Template;

use crate::iam::{
    IamManagedPolicyDetailView, IamManagedPolicyPrincipalView, IamManagedPolicySummary,
    IamPolicyAttachOption, IamPrincipalAccessKeyView, IamPrincipalDetailView,
    IamPrincipalInlinePolicyView, IamPrincipalSummary,
};
use crate::sqs::{MessageSummary, QueueAttribute, QueueSummary, QueueTag};
use crate::traces::{TraceDetailView, TraceSpanView, TraceSummaryView};

#[derive(Template)]
#[template(path = "home.html")]
pub(crate) struct HomeTemplate;

#[derive(Template)]
#[template(path = "error.html")]
pub(crate) struct ErrorTemplate<'a> {
    pub(crate) status_code: u16,
    pub(crate) title: &'a str,
    pub(crate) message: &'a str,
}

#[derive(Template)]
#[template(path = "traces/list.html")]
pub(crate) struct TraceListTemplate<'a> {
    pub(crate) page_header_html: &'a str,
    pub(crate) stats_html: &'a str,
    pub(crate) traces: &'a [TraceSummaryView],
    pub(crate) has_traces: bool,
}

#[derive(Template)]
#[template(path = "traces/detail.html")]
pub(crate) struct TraceDetailTemplate<'a> {
    pub(crate) page_header_html: &'a str,
    pub(crate) trace: &'a TraceDetailView,
    pub(crate) spans: &'a [TraceSpanView],
    pub(crate) has_spans: bool,
}

#[derive(Template)]
#[template(path = "iam/dashboard.html")]
pub(crate) struct IamDashboardTemplate<'a> {
    pub(crate) page_header_html: &'a str,
    pub(crate) stats_html: &'a str,
    pub(crate) empty_state_html: &'a str,
    pub(crate) feedback_message: &'a str,
    pub(crate) feedback_class: &'a str,
    pub(crate) has_feedback: bool,
    pub(crate) create_error: &'a str,
    pub(crate) has_create_error: bool,
    pub(crate) create_account_id: &'a str,
    pub(crate) create_kind: &'a str,
    pub(crate) create_name: &'a str,
    pub(crate) create_path: &'a str,
    pub(crate) principal_count: usize,
    pub(crate) principals: &'a [IamPrincipalSummary],
    pub(crate) has_principals: bool,
    pub(crate) managed_policy_count: usize,
    pub(crate) managed_policies: &'a [IamManagedPolicySummary],
    pub(crate) has_managed_policies: bool,
    pub(crate) policy_error: &'a str,
    pub(crate) has_policy_error: bool,
    pub(crate) policy_account_id: &'a str,
    pub(crate) policy_name: &'a str,
    pub(crate) policy_path: &'a str,
    pub(crate) policy_document: &'a str,
    pub(crate) policy_panel_open: bool,
}

#[derive(Template)]
#[template(path = "iam/principal_detail.html")]
pub(crate) struct IamPrincipalDetailTemplate<'a> {
    pub(crate) action_card_html: &'a str,
    pub(crate) summary_cards_html: &'a str,
    pub(crate) metadata_list_html: &'a str,
    pub(crate) feedback_message: &'a str,
    pub(crate) feedback_class: &'a str,
    pub(crate) has_feedback: bool,
    pub(crate) access_key_error: &'a str,
    pub(crate) has_access_key_error: bool,
    pub(crate) policy_error: &'a str,
    pub(crate) has_policy_error: bool,
    pub(crate) attach_policy_error: &'a str,
    pub(crate) has_attach_policy_error: bool,
    pub(crate) policy_name: &'a str,
    pub(crate) policy_document: &'a str,
    pub(crate) policy_panel_open: bool,
    pub(crate) principal: &'a IamPrincipalDetailView,
    pub(crate) access_keys: &'a [IamPrincipalAccessKeyView],
    pub(crate) inline_policies: &'a [IamPrincipalInlinePolicyView],
    pub(crate) attached_policies: &'a [IamManagedPolicySummary],
    pub(crate) policy_attach_options: &'a [IamPolicyAttachOption],
    pub(crate) has_access_keys: bool,
    pub(crate) has_inline_policies: bool,
    pub(crate) has_attached_policies: bool,
    pub(crate) has_policy_attach_options: bool,
}

#[derive(Template)]
#[template(path = "iam/managed_policy_detail.html")]
pub(crate) struct IamManagedPolicyDetailTemplate<'a> {
    pub(crate) action_card_html: &'a str,
    pub(crate) metadata_list_html: &'a str,
    pub(crate) feedback_message: &'a str,
    pub(crate) feedback_class: &'a str,
    pub(crate) has_feedback: bool,
    pub(crate) policy_error: &'a str,
    pub(crate) has_policy_error: bool,
    pub(crate) policy_document: &'a str,
    pub(crate) policy_panel_open: bool,
    pub(crate) policy: &'a IamManagedPolicyDetailView,
    pub(crate) attached_principals: &'a [IamManagedPolicyPrincipalView],
    pub(crate) attach_options: &'a [IamManagedPolicyPrincipalView],
    pub(crate) has_attached_principals: bool,
    pub(crate) has_attach_options: bool,
}

#[derive(Template)]
#[template(path = "sqs/dashboard.html")]
pub(crate) struct SqsDashboardTemplate<'a> {
    pub(crate) page_header_html: &'a str,
    pub(crate) dashboard_stats_html: &'a str,
    pub(crate) empty_state_html: &'a str,
    pub(crate) region: &'a str,
    pub(crate) account_id: &'a str,
    pub(crate) prefix: &'a str,
    pub(crate) return_to: &'a str,
    pub(crate) feedback_message: &'a str,
    pub(crate) feedback_class: &'a str,
    pub(crate) has_feedback: bool,
    pub(crate) create_error: &'a str,
    pub(crate) has_create_error: bool,
    pub(crate) create_queue_name: &'a str,
    pub(crate) create_queue_type: &'a str,
    pub(crate) create_region: &'a str,
    pub(crate) create_account_id: &'a str,
    pub(crate) queues: &'a [QueueSummary],
    pub(crate) has_queues: bool,
}

#[derive(Template)]
#[template(path = "sqs/queues.html")]
pub(crate) struct SqsQueuesTemplate<'a> {
    pub(crate) empty_state_html: &'a str,
    pub(crate) region: &'a str,
    pub(crate) account_id: &'a str,
    pub(crate) prefix: &'a str,
    pub(crate) return_to: &'a str,
    pub(crate) feedback_message: &'a str,
    pub(crate) feedback_class: &'a str,
    pub(crate) has_feedback: bool,
    pub(crate) create_error: &'a str,
    pub(crate) has_create_error: bool,
    pub(crate) create_queue_name: &'a str,
    pub(crate) create_queue_type: &'a str,
    pub(crate) create_region: &'a str,
    pub(crate) create_account_id: &'a str,
    pub(crate) queues: &'a [QueueSummary],
    pub(crate) has_queues: bool,
}

#[derive(Template)]
#[template(path = "sqs/queue_detail.html")]
pub(crate) struct SqsQueueDetailTemplate<'a> {
    pub(crate) action_card_html: &'a str,
    pub(crate) metadata_list_html: &'a str,
    pub(crate) queue_id: i64,
    pub(crate) queue: &'a hiraeth_store::sqs::SqsQueue,
    pub(crate) queue_arn: &'a str,
    pub(crate) queue_url: &'a str,
    pub(crate) feedback_message: &'a str,
    pub(crate) feedback_class: &'a str,
    pub(crate) has_feedback: bool,
    pub(crate) send_error: &'a str,
    pub(crate) has_send_error: bool,
    pub(crate) tag_error: &'a str,
    pub(crate) has_tag_error: bool,
    pub(crate) tag_key: &'a str,
    pub(crate) tag_value: &'a str,
    pub(crate) open_tags_panel: bool,
    pub(crate) summary: &'a QueueSummary,
    pub(crate) attributes: &'a [QueueAttribute],
    pub(crate) tags: &'a [QueueTag],
    pub(crate) messages: &'a [MessageSummary],
    pub(crate) tag_count: usize,
    pub(crate) has_tags: bool,
    pub(crate) has_messages: bool,
    pub(crate) message_limit: i64,
}

#[derive(Template)]
#[template(path = "sqs/fragments/dashboard_stats.html")]
pub(crate) struct SqsDashboardStatsTemplate<'a> {
    pub(crate) stats_cards_html: &'a str,
}

#[derive(Template)]
#[template(path = "sqs/fragments/queue_list.html")]
pub(crate) struct QueueListTemplate<'a> {
    pub(crate) empty_state_html: &'a str,
    pub(crate) queues: &'a [QueueSummary],
    pub(crate) has_queues: bool,
}

#[derive(Template)]
#[template(path = "sqs/fragments/queue_detail_stats.html")]
pub(crate) struct QueueDetailStatsTemplate<'a> {
    pub(crate) queue_id: i64,
    pub(crate) summary: &'a QueueSummary,
}

#[derive(Template)]
#[template(path = "sqs/fragments/message_list.html")]
pub(crate) struct QueueMessageListTemplate<'a> {
    pub(crate) queue_id: i64,
    pub(crate) messages: &'a [MessageSummary],
    pub(crate) has_messages: bool,
    pub(crate) message_limit: i64,
}

#[cfg(test)]
mod tests {
    use askama::Template;
    use chrono::NaiveDate;

    use super::{IamDashboardTemplate, QueueListTemplate};
    use crate::components::EmptyState;
    use crate::iam::{IamManagedPolicySummary, IamPrincipalSummary};
    use crate::sqs::QueueSummary;

    #[test]
    fn queue_list_template_escapes_queue_names() {
        let queues = vec![QueueSummary {
            id: 1,
            name: "<orders&billing>".to_string(),
            region: "us-east-1".to_string(),
            account_id: "000000000000".to_string(),
            queue_type: "standard".to_string(),
            message_count: 3,
            visible_message_count: 2,
            delayed_message_count: 1,
        }];

        let html = QueueListTemplate {
            empty_state_html: "",
            queues: &queues,
            has_queues: true,
        }
        .render()
        .expect("queue list template should render");

        assert!(html.contains("orders"));
        assert!(html.contains("billing"));
        assert!(!html.contains("<orders&billing>"));
    }

    #[test]
    fn iam_dashboard_template_escapes_principal_names() {
        let principals = vec![IamPrincipalSummary {
            id: 1,
            account_id: "000000000000".to_string(),
            kind: "user".to_string(),
            name: "<test&admin>".to_string(),
            created_at: NaiveDate::from_ymd_opt(2026, 4, 21)
                .expect("date should be valid")
                .and_hms_opt(12, 0, 0)
                .expect("time should be valid")
                .format("%Y-%m-%d %H:%M:%S UTC")
                .to_string(),
            access_key_count: 1,
            inline_policy_count: 2,
            managed_policy_count: 1,
        }];
        let managed_policies = Vec::<IamManagedPolicySummary>::new();
        let empty_state = EmptyState {
            title: "No principals".to_string(),
            message: "Seed some IAM data.".to_string(),
        }
        .render()
        .expect("empty state should render");

        let html = IamDashboardTemplate {
            page_header_html: "",
            stats_html: "",
            empty_state_html: &empty_state,
            feedback_message: "",
            feedback_class: "",
            has_feedback: false,
            create_error: "",
            has_create_error: false,
            create_account_id: "000000000000",
            create_kind: "user",
            create_name: "",
            create_path: "/",
            principal_count: principals.len(),
            principals: &principals,
            has_principals: true,
            managed_policy_count: managed_policies.len(),
            managed_policies: &managed_policies,
            has_managed_policies: false,
            policy_error: "",
            has_policy_error: false,
            policy_account_id: "000000000000",
            policy_name: "",
            policy_path: "/",
            policy_document: "{}",
            policy_panel_open: false,
        }
        .render()
        .expect("iam dashboard template should render");

        assert!(html.contains("test"));
        assert!(html.contains("admin"));
        assert!(!html.contains("<test&admin>"));
    }
}
