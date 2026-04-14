use std::collections::HashMap;

use hiraeth_auth::ResolvedRequest;
use hiraeth_router::ServiceResponse;
use hiraeth_store::sqs::{SqsQueue, SqsStore};
use serde::{Deserialize, Serialize};

use crate::{error::SqsError, util};

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct GetQueueAttributesRequest {
    pub queue_url: String,
    pub attribute_names: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "PascalCase")]
pub struct GetQueueAttributesResponse {
    pub attributes: HashMap<String, String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct QueueAttributeValues {
    pub visibility_timeout_seconds: i64,
    pub delay_seconds: i64,
    pub message_retention_period_seconds: i64,
    pub receive_message_wait_time_seconds: i64,
}

impl Default for QueueAttributeValues {
    fn default() -> Self {
        Self {
            visibility_timeout_seconds: 30,
            delay_seconds: 0,
            message_retention_period_seconds: 345600,
            receive_message_wait_time_seconds: 0,
        }
    }
}

impl QueueAttributeValues {
    pub(crate) fn from_attribute_map(attributes: &HashMap<String, String>) -> Self {
        let defaults = Self::default();

        Self {
            visibility_timeout_seconds: get_i64_attribute(
                attributes,
                "VisibilityTimeout",
                defaults.visibility_timeout_seconds,
            ),
            delay_seconds: get_i64_attribute(attributes, "DelaySeconds", defaults.delay_seconds),
            message_retention_period_seconds: get_i64_attribute(
                attributes,
                "MessageRetentionPeriod",
                defaults.message_retention_period_seconds,
            ),
            receive_message_wait_time_seconds: get_i64_attribute(
                attributes,
                "ReceiveMessageWaitTimeSeconds",
                defaults.receive_message_wait_time_seconds,
            ),
        }
    }
}

pub(crate) async fn get_queue_attributes<S: SqsStore>(
    request: &ResolvedRequest,
    store: &S,
) -> Result<ServiceResponse, SqsError> {
    let attributes_request = util::parse_request_body::<GetQueueAttributesRequest>(request)?;

    let queue = util::load_queue_from_url(request, store, &attributes_request.queue_url).await?;
    let attributes =
        collect_queue_attributes(store, &queue, &attributes_request.attribute_names).await?;

    Ok(ServiceResponse {
        status_code: 200,
        headers: vec![],
        body: serde_json::to_vec(&GetQueueAttributesResponse { attributes })
            .map_err(|e| SqsError::BadRequest(e.to_string()))?,
    })
}

pub(crate) async fn collect_queue_attributes<S: SqsStore>(
    store: &S,
    queue: &SqsQueue,
    requested_attributes: &[String],
) -> Result<HashMap<String, String>, SqsError> {
    let mut attributes = HashMap::<String, String>::new();

    insert_static_attribute(&mut attributes, requested_attributes, "Policy", "{}");
    insert_i64_attribute(
        &mut attributes,
        requested_attributes,
        "VisibilityTimeout",
        queue.visibility_timeout_seconds,
    );
    insert_static_attribute(
        &mut attributes,
        requested_attributes,
        "MaximumMessageSize",
        "1048576",
    );
    insert_i64_attribute(
        &mut attributes,
        requested_attributes,
        "MessageRetentionPeriod",
        queue.message_retention_period_seconds,
    );

    if is_requested_attribute("ApproximateNumberOfMessages", requested_attributes) {
        let message_count = store
            .get_message_count(queue.id)
            .await
            .map_err(|e| SqsError::InternalError(e.to_string()))?;
        attributes.insert(
            "ApproximateNumberOfMessages".to_string(),
            message_count.to_string(),
        );
    }

    if is_requested_attribute(
        "ApproximateNumberOfMessagesNotVisible",
        requested_attributes,
    ) {
        let visible_message_count = store
            .get_visible_message_count(queue.id)
            .await
            .map_err(|e| SqsError::InternalError(e.to_string()))?;
        let message_count = store
            .get_message_count(queue.id)
            .await
            .map_err(|e| SqsError::InternalError(e.to_string()))?;
        attributes.insert(
            "ApproximateNumberOfMessagesNotVisible".to_string(),
            (message_count - visible_message_count).to_string(),
        );
    }

    if is_requested_attribute("CreatedTimestamp", requested_attributes) {
        attributes.insert(
            "CreatedTimestamp".to_string(),
            queue.created_at.and_utc().timestamp_millis().to_string(),
        );
    }

    if is_requested_attribute("LastModifiedTimestamp", requested_attributes) {
        attributes.insert(
            "LastModifiedTimestamp".to_string(),
            queue.created_at.and_utc().timestamp_millis().to_string(),
        );
    }

    if is_requested_attribute("QueueArn", requested_attributes) {
        attributes.insert(
            "QueueArn".to_string(),
            format!(
                "arn:aws:sqs:{}:{}:{}",
                queue.region, queue.account_id, queue.name
            ),
        );
    }

    if is_requested_attribute("ApproximateNumberOfMessagesDelayed", requested_attributes) {
        let delayed_message_count = store
            .get_messages_delayed_count(queue.id)
            .await
            .map_err(|e| SqsError::InternalError(e.to_string()))?;
        attributes.insert(
            "ApproximateNumberOfMessagesDelayed".to_string(),
            delayed_message_count.to_string(),
        );
    }

    insert_i64_attribute(
        &mut attributes,
        requested_attributes,
        "DelaySeconds",
        queue.delay_seconds,
    );
    insert_i64_attribute(
        &mut attributes,
        requested_attributes,
        "ReceiveMessageWaitTimeSeconds",
        queue.receive_message_wait_time_seconds,
    );
    insert_static_attribute(&mut attributes, requested_attributes, "RedrivePolicy", "{}");
    insert_static_attribute(
        &mut attributes,
        requested_attributes,
        "RedriveAllowPolicy",
        "{}",
    );
    insert_static_attribute(
        &mut attributes,
        requested_attributes,
        "SqsManagedSseEnabled",
        "false",
    );

    Ok(attributes)
}

fn get_i64_attribute(attributes: &HashMap<String, String>, name: &str, default: i64) -> i64 {
    attributes
        .get(name)
        .and_then(|value| value.parse::<i64>().ok())
        .unwrap_or(default)
}

fn insert_i64_attribute(
    attributes: &mut HashMap<String, String>,
    requested_attributes: &[String],
    name: &str,
    value: i64,
) {
    if is_requested_attribute(name, requested_attributes) {
        attributes.insert(name.to_string(), value.to_string());
    }
}

fn insert_static_attribute(
    attributes: &mut HashMap<String, String>,
    requested_attributes: &[String],
    name: &str,
    value: &str,
) {
    if is_requested_attribute(name, requested_attributes) {
        attributes.insert(name.to_string(), value.to_string());
    }
}

fn is_requested_attribute(attribute_name: &str, requested_attributes: &[String]) -> bool {
    requested_attributes
        .iter()
        .any(|requested| requested == attribute_name || requested == "All")
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use chrono::{TimeZone, Utc};
    use hiraeth_auth::{AuthContext, ResolvedRequest};
    use hiraeth_http::IncomingRequest;
    use hiraeth_router::ServiceResponse;
    use hiraeth_store::{principal::Principal, sqs::SqsQueue, test_support::SqsTestStore};
    use serde_json::Value;

    use super::{QueueAttributeValues, collect_queue_attributes, get_queue_attributes};
    use crate::error::SqsError;

    fn resolved_request(body: &str) -> ResolvedRequest {
        let mut headers = HashMap::new();
        headers.insert(
            "x-amz-target".to_string(),
            "AmazonSQS.GetQueueAttributes".to_string(),
        );

        ResolvedRequest {
            request: IncomingRequest {
                host: "localhost:4566".to_string(),
                method: "POST".to_string(),
                path: "/".to_string(),
                query: None,
                headers,
                body: body.as_bytes().to_vec(),
            },
            service: "sqs".to_string(),
            region: "us-east-1".to_string(),
            auth_context: AuthContext {
                access_key: "AKIAIOSFODNN7EXAMPLE".to_string(),
                principal: Principal {
                    id: 1,
                    account_id: "123456789012".to_string(),
                    kind: "user".to_string(),
                    name: "test-user".to_string(),
                    created_at: Utc
                        .with_ymd_and_hms(2026, 4, 4, 12, 0, 0)
                        .unwrap()
                        .naive_utc(),
                },
            },
            date: Utc.with_ymd_and_hms(2026, 4, 4, 12, 0, 0).unwrap(),
        }
    }

    fn queue() -> SqsQueue {
        SqsQueue {
            id: 42,
            name: "orders".to_string(),
            region: "us-east-1".to_string(),
            account_id: "123456789012".to_string(),
            queue_type: "standard".to_string(),
            visibility_timeout_seconds: 30,
            delay_seconds: 5,
            message_retention_period_seconds: 345600,
            receive_message_wait_time_seconds: 10,
            created_at: Utc
                .with_ymd_and_hms(2026, 4, 4, 11, 0, 0)
                .unwrap()
                .naive_utc(),
        }
    }

    fn parse_json_body(response: &ServiceResponse) -> Value {
        serde_json::from_slice(&response.body).expect("response body should be valid json")
    }

    fn attribute_names(names: &[&str]) -> Vec<String> {
        names.iter().map(|name| name.to_string()).collect()
    }

    #[test]
    fn queue_attribute_values_uses_defaults_for_missing_attributes() {
        let attributes = QueueAttributeValues::from_attribute_map(&HashMap::new());

        assert_eq!(attributes, QueueAttributeValues::default());
    }

    #[test]
    fn queue_attribute_values_parses_supported_attributes() {
        let attributes = QueueAttributeValues::from_attribute_map(&HashMap::from([
            ("VisibilityTimeout".to_string(), "45".to_string()),
            ("DelaySeconds".to_string(), "5".to_string()),
            ("MessageRetentionPeriod".to_string(), "86400".to_string()),
            (
                "ReceiveMessageWaitTimeSeconds".to_string(),
                "10".to_string(),
            ),
        ]));

        assert_eq!(
            attributes,
            QueueAttributeValues {
                visibility_timeout_seconds: 45,
                delay_seconds: 5,
                message_retention_period_seconds: 86400,
                receive_message_wait_time_seconds: 10,
            }
        );
    }

    #[test]
    fn queue_attribute_values_falls_back_to_defaults_for_invalid_values() {
        let attributes = QueueAttributeValues::from_attribute_map(&HashMap::from([
            ("VisibilityTimeout".to_string(), "not-a-number".to_string()),
            ("DelaySeconds".to_string(), "5".to_string()),
        ]));

        assert_eq!(
            attributes,
            QueueAttributeValues {
                delay_seconds: 5,
                ..QueueAttributeValues::default()
            }
        );
    }

    #[tokio::test]
    async fn collect_queue_attributes_returns_all_supported_attributes() {
        let store = SqsTestStore::with_queue(queue()).with_message_counts(7, 3, 2);

        let attributes = collect_queue_attributes(&store, &queue(), &attribute_names(&["All"]))
            .await
            .expect("attributes should collect");

        assert_eq!(attributes["Policy"], "{}");
        assert_eq!(attributes["VisibilityTimeout"], "30");
        assert_eq!(attributes["MaximumMessageSize"], "1048576");
        assert_eq!(attributes["MessageRetentionPeriod"], "345600");
        assert_eq!(attributes["ApproximateNumberOfMessages"], "7");
        assert_eq!(attributes["ApproximateNumberOfMessagesNotVisible"], "4");
        assert_eq!(attributes["ApproximateNumberOfMessagesDelayed"], "2");
        assert_eq!(
            attributes["CreatedTimestamp"],
            Utc.with_ymd_and_hms(2026, 4, 4, 11, 0, 0)
                .unwrap()
                .timestamp_millis()
                .to_string()
        );
        assert_eq!(
            attributes["LastModifiedTimestamp"],
            Utc.with_ymd_and_hms(2026, 4, 4, 11, 0, 0)
                .unwrap()
                .timestamp_millis()
                .to_string()
        );
        assert_eq!(
            attributes["QueueArn"],
            "arn:aws:sqs:us-east-1:123456789012:orders"
        );
        assert_eq!(attributes["DelaySeconds"], "5");
        assert_eq!(attributes["ReceiveMessageWaitTimeSeconds"], "10");
        assert_eq!(attributes["RedrivePolicy"], "{}");
        assert_eq!(attributes["RedriveAllowPolicy"], "{}");
        assert_eq!(attributes["SqsManagedSseEnabled"], "false");
    }

    #[tokio::test]
    async fn get_queue_attributes_returns_requested_attributes() {
        let store = SqsTestStore::with_queue(queue()).with_message_counts(7, 3, 2);
        let request = resolved_request(
            r#"{
                "QueueUrl":"http://localhost:4566/123456789012/orders",
                "AttributeNames":[
                    "VisibilityTimeout",
                    "ApproximateNumberOfMessages",
                    "ApproximateNumberOfMessagesNotVisible",
                    "ApproximateNumberOfMessagesDelayed",
                    "QueueArn",
                    "CreatedTimestamp",
                    "ReceiveMessageWaitTimeSeconds"
                ]
            }"#,
        );

        let response = get_queue_attributes(&request, &store)
            .await
            .expect("get queue attributes should succeed");

        assert_eq!(response.status_code, 200);

        let body = parse_json_body(&response);
        let attributes = &body["Attributes"];
        assert_eq!(attributes["VisibilityTimeout"], "30");
        assert_eq!(attributes["ApproximateNumberOfMessages"], "7");
        assert_eq!(attributes["ApproximateNumberOfMessagesNotVisible"], "4");
        assert_eq!(attributes["ApproximateNumberOfMessagesDelayed"], "2");
        assert_eq!(
            attributes["QueueArn"],
            "arn:aws:sqs:us-east-1:123456789012:orders"
        );
        assert_eq!(
            attributes["CreatedTimestamp"],
            Utc.with_ymd_and_hms(2026, 4, 4, 11, 0, 0)
                .unwrap()
                .timestamp_millis()
                .to_string()
        );
        assert_eq!(attributes["ReceiveMessageWaitTimeSeconds"], "10");
    }

    #[tokio::test]
    async fn get_queue_attributes_returns_not_found_for_missing_queue() {
        let store = SqsTestStore::default();
        let request = resolved_request(
            r#"{
                "QueueUrl":"http://localhost:4566/123456789012/orders",
                "AttributeNames":["All"]
            }"#,
        );

        let result = get_queue_attributes(&request, &store).await;

        assert!(matches!(result, Err(SqsError::QueueNotFound)));
    }
}
