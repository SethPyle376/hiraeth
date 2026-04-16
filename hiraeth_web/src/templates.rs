use askama::Template;

use crate::sqs::{MessageSummary, QueueAttribute, QueueSummary, QueueTag};

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
#[template(path = "sqs/dashboard.html")]
pub(crate) struct SqsDashboardTemplate<'a> {
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
    pub(crate) total_queues: usize,
    pub(crate) total_messages: i64,
    pub(crate) visible_messages: i64,
    pub(crate) delayed_messages: i64,
}

#[derive(Template)]
#[template(path = "sqs/queues.html")]
pub(crate) struct SqsQueuesTemplate<'a> {
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
    pub(crate) region: &'a str,
    pub(crate) account_id: &'a str,
    pub(crate) total_queues: usize,
    pub(crate) total_messages: i64,
    pub(crate) visible_messages: i64,
    pub(crate) delayed_messages: i64,
}

#[derive(Template)]
#[template(path = "sqs/fragments/queue_list.html")]
pub(crate) struct QueueListTemplate<'a> {
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

    use super::QueueListTemplate;
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
            queues: &queues,
            has_queues: true,
        }
        .render()
        .expect("queue list template should render");

        assert!(html.contains("orders"));
        assert!(html.contains("billing"));
        assert!(!html.contains("<orders&billing>"));
    }
}
