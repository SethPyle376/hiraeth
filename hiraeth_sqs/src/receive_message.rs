use std::collections::BTreeMap;

use hiraeth_auth::ResolvedRequest;
use hiraeth_router::ServiceResponse;
use hiraeth_store::{StoreError, sqs::SqsStore};
use serde::{Deserialize, Serialize};

use crate::{SqsError, util};

fn default_max_number_of_messages() -> i64 {
    1
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct ReceiveMessageRequest {
    #[serde(default)]
    pub attribute_names: Vec<String>,
    #[serde(default = "default_max_number_of_messages")]
    pub max_number_of_messages: i64,
    #[serde(default)]
    pub message_attribute_names: Vec<String>,
    #[serde(default)]
    pub message_system_attribute_names: Vec<String>,
    pub queue_url: String,
    pub visibility_timeout: Option<u32>,
    pub wait_time_seconds: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "PascalCase")]
pub struct ReceivedMessage {
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub attributes: BTreeMap<String, String>,
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub message_attributes: BTreeMap<String, util::MessageAttributeValue>,
    pub message_id: String,
    pub receipt_handle: String,
    pub body: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(rename = "MD5OfBody")]
    pub md5_of_body: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(rename = "MD5OfMessageAttributes")]
    pub md5_of_message_attributes: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "PascalCase")]
pub struct ReceiveMessageResponse {
    pub messages: Vec<ReceivedMessage>,
}

pub(crate) async fn receive_message<S: SqsStore>(
    request: &ResolvedRequest,
    store: &S,
) -> Result<ServiceResponse, SqsError> {
    let receive_request = serde_json::from_str::<ReceiveMessageRequest>(
        String::from_utf8(request.request.body.clone())
            .map_err(|e| SqsError::BadRequest(e.to_string()))?
            .as_str(),
    )
    .map_err(|e| SqsError::BadRequest(e.to_string()))?;

    let queue_id = util::parse_queue_url(&receive_request.queue_url, &request.region)
        .ok_or_else(|| SqsError::BadRequest("Invalid queue url".to_string()))?;

    let queue = store
        .get_queue(&queue_id.name, &queue_id.region, &queue_id.account_id)
        .await
        .map_err(|e| SqsError::StoreError(e))?
        .ok_or_else(|| {
            SqsError::QueueNotFound(
                queue_id.name.clone(),
                queue_id.region.clone(),
                queue_id.account_id.clone(),
            )
        })?;

    let visibility_timeout_seconds = receive_request
        .visibility_timeout
        .unwrap_or_else(|| queue.visibility_timeout_seconds as u32);

    let messages = store
        .receive_messages(
            queue.id,
            receive_request.max_number_of_messages,
            visibility_timeout_seconds,
        )
        .await
        .map_err(|e| SqsError::StoreError(e))?;

    let received_messages = messages
        .into_iter()
        .map(|msg| {
            let attributes = select_system_attributes(&receive_request, &msg);
            let message_attributes = filter_message_attributes(
                parse_message_attributes(&msg)?,
                &receive_request.message_attribute_names,
            );
            let md5_of_message_attributes = if message_attributes.is_empty() {
                None
            } else {
                Some(util::calculate_message_attributes_md5(&message_attributes)?)
            };

            Ok(ReceivedMessage {
                attributes,
                message_attributes,
                message_id: msg.message_id,
                receipt_handle: msg.receipt_handle.unwrap_or_default(),
                body: msg.body.clone(),
                md5_of_body: Some(format!("{:x}", md5::compute(msg.body.as_bytes()))),
                md5_of_message_attributes,
            })
        })
        .collect::<Result<Vec<_>, SqsError>>()?;

    let response = ReceiveMessageResponse {
        messages: received_messages,
    };
    Ok(ServiceResponse {
        status_code: 200,
        headers: vec![],
        body: serde_json::to_vec(&response).unwrap_or_default(),
    })
}

fn epoch_millis(value: chrono::NaiveDateTime) -> String {
    value.and_utc().timestamp_millis().to_string()
}

fn should_include_system_attribute(attribute_name: &str, request: &ReceiveMessageRequest) -> bool {
    request
        .attribute_names
        .iter()
        .any(|name| name == "All" || name == attribute_name)
        || request
            .message_system_attribute_names
            .iter()
            .any(|name| name == "All" || name == attribute_name)
}

fn select_system_attributes(
    request: &ReceiveMessageRequest,
    message: &hiraeth_store::sqs::SqsMessage,
) -> BTreeMap<String, String> {
    let mut attributes = BTreeMap::new();

    if should_include_system_attribute("ApproximateReceiveCount", request) {
        attributes.insert(
            "ApproximateReceiveCount".to_string(),
            message.receive_count.to_string(),
        );
    }

    if should_include_system_attribute("ApproximateFirstReceiveTimestamp", request) {
        if let Some(first_received_at) = message.first_received_at {
            attributes.insert(
                "ApproximateFirstReceiveTimestamp".to_string(),
                epoch_millis(first_received_at),
            );
        }
    }

    if should_include_system_attribute("SentTimestamp", request) {
        attributes.insert("SentTimestamp".to_string(), epoch_millis(message.sent_at));
    }

    if should_include_system_attribute("AWSTraceHeader", request) {
        if let Some(aws_trace_header) = &message.aws_trace_header {
            attributes.insert("AWSTraceHeader".to_string(), aws_trace_header.clone());
        }
    }

    if should_include_system_attribute("MessageDeduplicationId", request) {
        if let Some(message_deduplication_id) = &message.message_deduplication_id {
            attributes.insert(
                "MessageDeduplicationId".to_string(),
                message_deduplication_id.clone(),
            );
        }
    }

    if should_include_system_attribute("MessageGroupId", request) {
        if let Some(message_group_id) = &message.message_group_id {
            attributes.insert("MessageGroupId".to_string(), message_group_id.clone());
        }
    }

    attributes
}

fn parse_message_attributes(
    message: &hiraeth_store::sqs::SqsMessage,
) -> Result<BTreeMap<String, util::MessageAttributeValue>, SqsError> {
    match message.message_attributes.as_deref() {
        Some(raw) if !raw.is_empty() => serde_json::from_str(raw).map_err(|e| {
            SqsError::StoreError(StoreError::StorageFailure(format!(
                "failed to parse stored message attributes for message {}: {}",
                message.message_id, e
            )))
        }),
        _ => Ok(BTreeMap::new()),
    }
}

fn include_requested_message_attribute(attribute_name: &str, requested_names: &[String]) -> bool {
    if requested_names.is_empty() {
        return false;
    }

    requested_names.iter().any(|requested_name| {
        requested_name == "All"
            || requested_name == ".*"
            || requested_name == attribute_name
            || requested_name
                .strip_suffix(".*")
                .is_some_and(|prefix| attribute_name.starts_with(prefix))
    })
}

fn filter_message_attributes(
    message_attributes: BTreeMap<String, util::MessageAttributeValue>,
    requested_names: &[String],
) -> BTreeMap<String, util::MessageAttributeValue> {
    message_attributes
        .into_iter()
        .filter(|(name, _)| include_requested_message_attribute(name, requested_names))
        .collect()
}

#[cfg(test)]
mod tests {
    use std::{collections::HashMap, sync::Mutex};

    use async_trait::async_trait;
    use chrono::{TimeZone, Utc};
    use hiraeth_auth::{AuthContext, ResolvedRequest};
    use hiraeth_http::IncomingRequest;
    use hiraeth_router::ServiceResponse;
    use hiraeth_store::{
        StoreError,
        principal::Principal,
        sqs::{SqsMessage, SqsQueue, SqsStore},
    };
    use serde_json::Value;

    use super::receive_message;
    use crate::SqsError;

    struct TestSqsStore {
        queue: SqsQueue,
        messages: Mutex<Vec<SqsMessage>>,
    }

    impl TestSqsStore {
        fn new(queue: SqsQueue, messages: Vec<SqsMessage>) -> Self {
            Self {
                queue,
                messages: Mutex::new(messages),
            }
        }
    }

    #[async_trait]
    impl SqsStore for TestSqsStore {
        async fn create_queue(&self, _queue: SqsQueue) -> Result<(), StoreError> {
            unimplemented!()
        }

        async fn delete_queue(&self, _queue_id: i64) -> Result<(), StoreError> {
            unimplemented!()
        }

        async fn get_queue(
            &self,
            queue_name: &str,
            region: &str,
            account_id: &str,
        ) -> Result<Option<SqsQueue>, StoreError> {
            Ok((self.queue.name == queue_name
                && self.queue.region == region
                && self.queue.account_id == account_id)
                .then(|| self.queue.clone()))
        }

        async fn get_message_count(&self, _queue_id: i64) -> Result<i64, StoreError> {
            unimplemented!()
        }

        async fn get_visible_message_count(&self, _queue_id: i64) -> Result<i64, StoreError> {
            unimplemented!()
        }

        async fn get_messages_delayed_count(&self, _queue_id: i64) -> Result<i64, StoreError> {
            unimplemented!()
        }

        async fn send_message(&self, _message: &SqsMessage) -> Result<(), StoreError> {
            unimplemented!()
        }

        async fn receive_messages(
            &self,
            queue_id: i64,
            max_number_of_messages: i64,
            _visibility_timeout_seconds: u32,
        ) -> Result<Vec<SqsMessage>, StoreError> {
            let messages = self.messages.lock().expect("messages mutex");
            Ok(messages
                .iter()
                .filter(|message| message.queue_id == queue_id)
                .take(max_number_of_messages as usize)
                .cloned()
                .collect())
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
            delay_seconds: 0,
            message_retention_period_seconds: 345600,
            receive_message_wait_time_seconds: 0,
            created_at: Utc
                .with_ymd_and_hms(2026, 4, 3, 12, 0, 0)
                .unwrap()
                .naive_utc(),
        }
    }

    fn resolved_request(body: &str) -> ResolvedRequest {
        let mut headers = HashMap::new();
        headers.insert(
            "x-amz-target".to_string(),
            "AmazonSQS.ReceiveMessage".to_string(),
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
                        .with_ymd_and_hms(2026, 4, 3, 12, 0, 0)
                        .unwrap()
                        .naive_utc(),
                },
            },
            date: Utc.with_ymd_and_hms(2026, 4, 3, 12, 0, 0).unwrap(),
        }
    }

    fn message(queue_id: i64) -> SqsMessage {
        SqsMessage {
            message_id: "msg-123".to_string(),
            queue_id,
            body: "hello world".to_string(),
            message_attributes: Some(
                r#"{"trace_id":{"DataType":"String","StringValue":"abc123","BinaryValue":null},"tenant":{"DataType":"String","StringValue":"acme","BinaryValue":null}}"#
                    .to_string(),
            ),
            aws_trace_header: Some("Root=1-abcdef12-0123456789abcdef01234567".to_string()),
            sent_at: Utc
                .with_ymd_and_hms(2026, 4, 3, 11, 59, 0)
                .unwrap()
                .naive_utc(),
            visible_at: Utc
                .with_ymd_and_hms(2026, 4, 3, 12, 0, 30)
                .unwrap()
                .naive_utc(),
            expires_at: Utc
                .with_ymd_and_hms(2026, 4, 7, 12, 0, 0)
                .unwrap()
                .naive_utc(),
            receive_count: 2,
            receipt_handle: Some("receipt-123".to_string()),
            first_received_at: Some(
                Utc.with_ymd_and_hms(2026, 4, 3, 12, 0, 0)
                    .unwrap()
                    .naive_utc(),
            ),
            message_group_id: Some("group-1".to_string()),
            message_deduplication_id: Some("dedupe-1".to_string()),
        }
    }

    fn parse_json_body(response: &ServiceResponse) -> Value {
        serde_json::from_slice(&response.body).expect("response body should be valid json")
    }

    #[tokio::test]
    async fn receive_message_returns_requested_message_and_system_attributes() {
        let store = TestSqsStore::new(queue(), vec![message(42)]);
        let request = resolved_request(
            r#"{
                "QueueUrl":"http://localhost:4566/123456789012/orders",
                "MessageAttributeNames":["All"],
                "MessageSystemAttributeNames":["ApproximateReceiveCount","ApproximateFirstReceiveTimestamp","SentTimestamp","MessageGroupId","MessageDeduplicationId","AWSTraceHeader"]
            }"#,
        );

        let response = receive_message(&request, &store)
            .await
            .expect("receive message should succeed");

        assert_eq!(response.status_code, 200);

        let body = parse_json_body(&response);
        let messages = body["Messages"]
            .as_array()
            .expect("Messages should be an array");
        assert_eq!(messages.len(), 1);

        let message = &messages[0];
        assert_eq!(message["MessageId"], "msg-123");
        assert_eq!(message["ReceiptHandle"], "receipt-123");
        assert_eq!(message["Body"], "hello world");
        assert_eq!(message["MD5OfBody"], "5eb63bbbe01eeed093cb22bb8f5acdc3");
        assert_eq!(
            message["MD5OfMessageAttributes"],
            "dbf9f8110dff50952a8b7b0d4fc539f2"
        );

        assert_eq!(message["Attributes"]["ApproximateReceiveCount"], "2");
        assert_eq!(
            message["Attributes"]["ApproximateFirstReceiveTimestamp"],
            "1775217600000"
        );
        assert_eq!(message["Attributes"]["SentTimestamp"], "1775217540000");
        assert_eq!(
            message["Attributes"]["AWSTraceHeader"],
            "Root=1-abcdef12-0123456789abcdef01234567"
        );
        assert_eq!(message["Attributes"]["MessageGroupId"], "group-1");
        assert_eq!(message["Attributes"]["MessageDeduplicationId"], "dedupe-1");

        assert_eq!(
            message["MessageAttributes"]["trace_id"]["StringValue"],
            "abc123"
        );
        assert_eq!(
            message["MessageAttributes"]["tenant"]["StringValue"],
            "acme"
        );
    }

    #[tokio::test]
    async fn receive_message_filters_requested_message_attributes() {
        let store = TestSqsStore::new(queue(), vec![message(42)]);
        let request = resolved_request(
            r#"{
                "QueueUrl":"http://localhost:4566/123456789012/orders",
                "AttributeNames":["SentTimestamp"],
                "MessageAttributeNames":["trace_id"]
            }"#,
        );

        let response = receive_message(&request, &store)
            .await
            .expect("receive message should succeed");

        let body = parse_json_body(&response);
        let message = &body["Messages"][0];

        assert_eq!(message["Attributes"]["SentTimestamp"], "1775217540000");
        assert!(
            message["Attributes"]
                .get("ApproximateReceiveCount")
                .is_none()
        );
        assert_eq!(
            message["MessageAttributes"]["trace_id"]["StringValue"],
            "abc123"
        );
        assert!(message["MessageAttributes"].get("tenant").is_none());
        assert_eq!(
            message["MD5OfMessageAttributes"],
            "853c383c82274bde6eac88d91ee96efe"
        );
    }

    #[tokio::test]
    async fn receive_message_returns_queue_not_found_for_missing_queue() {
        let mut missing_queue = queue();
        missing_queue.name = "other".to_string();
        let store = TestSqsStore::new(missing_queue, vec![message(42)]);
        let request =
            resolved_request(r#"{"QueueUrl":"http://localhost:4566/123456789012/orders"}"#);

        let result = receive_message(&request, &store).await;

        assert!(matches!(
            result,
            Err(SqsError::QueueNotFound(name, region, account))
                if name == "orders" && region == "us-east-1" && account == "123456789012"
        ));
    }
}
