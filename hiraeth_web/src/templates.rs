use askama::Template;

use crate::sqs::{MessageSummary, QueueAttribute, QueueSummary};

#[derive(Template)]
#[template(path = "home.html")]
pub(crate) struct HomeTemplate;

#[derive(Template)]
#[template(path = "error.html")]
pub(crate) struct ErrorTemplate<'a> {
    pub(crate) message: &'a str,
}

#[derive(Template)]
#[template(path = "sqs/dashboard.html")]
pub(crate) struct SqsDashboardTemplate<'a> {
    pub(crate) region: &'a str,
    pub(crate) account_id: &'a str,
    pub(crate) prefix: &'a str,
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
    pub(crate) queues: &'a [QueueSummary],
    pub(crate) has_queues: bool,
}

#[derive(Template)]
#[template(path = "sqs/queue_detail.html")]
pub(crate) struct SqsQueueDetailTemplate<'a> {
    pub(crate) queue: &'a hiraeth_store::sqs::SqsQueue,
    pub(crate) summary: &'a QueueSummary,
    pub(crate) attributes: &'a [QueueAttribute],
    pub(crate) messages: &'a [MessageSummary],
    pub(crate) has_messages: bool,
    pub(crate) message_limit: i64,
}

#[derive(Template)]
#[template(path = "sqs/fragments/queue_list.html")]
pub(crate) struct QueueListTemplate<'a> {
    pub(crate) queues: &'a [QueueSummary],
    pub(crate) has_queues: bool,
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
