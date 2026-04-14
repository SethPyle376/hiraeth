use std::collections::HashMap;

use hiraeth_auth::ResolvedRequest;
use hiraeth_router::ServiceResponse;
use hiraeth_store::sqs::SqsStore;
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

pub(crate) async fn get_queue_attributes<S: SqsStore>(
    request: &ResolvedRequest,
    store: &S,
) -> Result<ServiceResponse, SqsError> {
    let attributes_request = util::parse_request_body::<GetQueueAttributesRequest>(request)?;

    let mut attributes = HashMap::<String, String>::new();
    let queue = util::load_queue_from_url(request, store, &attributes_request.queue_url).await?;

    if is_requested_attribute("Policy", &attributes_request.attribute_names) {
        attributes.insert("Policy".to_string(), "{}".to_string());
    }

    if is_requested_attribute("VisibilityTimeout", &attributes_request.attribute_names) {
        attributes.insert(
            "VisibilityTimeout".to_string(),
            queue.visibility_timeout_seconds.to_string(),
        );
    }

    if is_requested_attribute("MaximumMessageSize", &attributes_request.attribute_names) {
        attributes.insert("MaximumMessageSize".to_string(), "1048576".to_string());
    }

    if is_requested_attribute(
        "MessageRetentionPeriod",
        &attributes_request.attribute_names,
    ) {
        attributes.insert(
            "MessageRetentionPeriod".to_string(),
            queue.message_retention_period_seconds.to_string(),
        );
    }

    if is_requested_attribute(
        "ApproximateNumberOfMessages",
        &attributes_request.attribute_names,
    ) {
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
        &attributes_request.attribute_names,
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

    if is_requested_attribute("CreatedTimestamp", &attributes_request.attribute_names) {
        attributes.insert(
            "CreatedTimestamp".to_string(),
            queue.created_at.and_utc().timestamp_millis().to_string(),
        );
    }

    if is_requested_attribute("LastModifiedTimestamp", &attributes_request.attribute_names) {
        attributes.insert(
            "LastModifiedTimestamp".to_string(),
            queue.created_at.and_utc().timestamp_millis().to_string(),
        );
    }

    if is_requested_attribute("QueueArn", &attributes_request.attribute_names) {
        attributes.insert(
            "QueueArn".to_string(),
            format!(
                "arn:aws:sqs:{}:{}:{}",
                queue.region, queue.account_id, queue.name
            ),
        );
    }

    if is_requested_attribute(
        "ApproximateNumberOfMessagesDelayed",
        &attributes_request.attribute_names,
    ) {
        let delayed_message_count = store
            .get_messages_delayed_count(queue.id)
            .await
            .map_err(|e| SqsError::InternalError(e.to_string()))?;
        attributes.insert(
            "ApproximateNumberOfMessagesDelayed".to_string(),
            delayed_message_count.to_string(),
        );
    }

    if is_requested_attribute("DelaySeconds", &attributes_request.attribute_names) {
        attributes.insert("DelaySeconds".to_string(), queue.delay_seconds.to_string());
    }

    if is_requested_attribute(
        "ReceiveMessageWaitTimeSeconds",
        &attributes_request.attribute_names,
    ) {
        attributes.insert(
            "ReceiveMessageWaitTimeSeconds".to_string(),
            queue.receive_message_wait_time_seconds.to_string(),
        );
    }

    if is_requested_attribute("RedrivePolicy", &attributes_request.attribute_names) {
        attributes.insert("RedrivePolicy".to_string(), "{}".to_string());
    }

    if is_requested_attribute("RedriveAllowPolicy", &attributes_request.attribute_names) {
        attributes.insert("RedriveAllowPolicy".to_string(), "{}".to_string());
    }

    if is_requested_attribute("SqsManagedSseEnabled", &attributes_request.attribute_names) {
        attributes.insert("SqsManagedSseEnabled".to_string(), "false".to_string());
    }

    Ok(ServiceResponse {
        status_code: 200,
        headers: vec![],
        body: serde_json::to_vec(&GetQueueAttributesResponse { attributes })
            .map_err(|e| SqsError::BadRequest(e.to_string()))?,
    })
}

fn is_requested_attribute(attribute_name: &str, requested_attributes: &Vec<String>) -> bool {
    requested_attributes.contains(&attribute_name.to_string())
        || requested_attributes.contains(&"All".to_string())
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

    use super::get_queue_attributes;
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
