use std::collections::BTreeMap;

use hiraeth_auth::ResolvedRequest;
use hiraeth_router::ServiceResponse;
use hiraeth_store::sqs::{SqsMessage, SqsStore};
use serde::{Deserialize, Serialize};

use crate::{SqsError, util};

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct SendMessageRequest {
    delay_seconds: Option<i64>,
    #[serde(default)]
    message_attributes: Option<BTreeMap<String, util::MessageAttributeValue>>,
    message_body: String,
    message_deduplication_id: Option<String>,
    message_group_id: Option<String>,
    queue_url: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "PascalCase")]
struct SendMessageResponse {
    #[serde(rename = "MD5OfMessageAttributes")]
    #[serde(skip_serializing_if = "Option::is_none")]
    md5_of_message_attributes: Option<String>,
    #[serde(rename = "MD5OfMessageBody")]
    md5_of_message_body: String,
    message_id: String,
    sequence_number: Option<String>,
}

pub(crate) async fn send_message<S: SqsStore>(
    request: &ResolvedRequest,
    store: &S,
) -> Result<ServiceResponse, SqsError> {
    let request_body = serde_json::from_str::<SendMessageRequest>(
        String::from_utf8(request.request.body.clone())
            .map_err(|e| SqsError::BadRequest(e.to_string()))?
            .as_str(),
    )
    .map_err(|e| SqsError::BadRequest(e.to_string()))?;

    let queue_id = util::parse_queue_url(&request_body.queue_url, &request.region)
        .ok_or_else(|| SqsError::BadRequest("Invalid queue URL".to_string()))?;

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

    let visible_at = request.date.naive_utc()
        + chrono::Duration::seconds(request_body.delay_seconds.unwrap_or(queue.delay_seconds));

    let expires_at = request.date.naive_utc()
        + chrono::Duration::seconds(queue.message_retention_period_seconds);

    let message_attributes = request_body
        .message_attributes
        .as_ref()
        .map(|attrs| serde_json::to_string(attrs).map_err(|e| SqsError::BadRequest(e.to_string())))
        .transpose()?;

    let md5_of_message_attributes = request_body
        .message_attributes
        .as_ref()
        .filter(|attrs| !attrs.is_empty())
        .map(util::calculate_message_attributes_md5)
        .transpose()?;

    let message = SqsMessage {
        message_id: uuid::Uuid::new_v4().to_string(),
        queue_id: queue.id,
        body: request_body.message_body.clone(),
        message_attributes,
        sent_at: request.date.naive_utc(),
        visible_at,
        expires_at,
        receive_count: 0,
        receipt_handle: Option::None,
        first_received_at: Option::None,
        message_group_id: request_body.message_group_id.clone(),
        message_deduplication_id: request_body.message_deduplication_id.clone(),
    };

    store
        .send_message(&message)
        .await
        .map_err(|e| SqsError::StoreError(e))?;

    let response = SendMessageResponse {
        md5_of_message_attributes,
        md5_of_message_body: format!("{:x}", md5::compute(request_body.message_body.as_bytes())),
        message_id: message.message_id.clone(),
        sequence_number: None,
    };

    Ok(ServiceResponse {
        status_code: 200,
        headers: vec![],
        body: serde_json::to_vec(&response).unwrap_or_default(),
    })
}

#[cfg(test)]
mod tests {
    use std::{collections::HashMap, sync::Mutex};

    use async_trait::async_trait;
    use chrono::{TimeZone, Utc};
    use hiraeth_auth::{AuthContext, ResolvedRequest};
    use hiraeth_router::ServiceResponse;
    use hiraeth_store::{
        StoreError,
        principal::Principal,
        sqs::{SqsMessage, SqsQueue, SqsStore},
    };
    use serde_json::Value;

    use super::send_message;
    use crate::{SqsError, util::MessageAttributeValue};

    #[derive(Default)]
    struct TestSqsStore {
        queue: Mutex<Option<SqsQueue>>,
        sent_messages: Mutex<Vec<SqsMessage>>,
    }

    impl TestSqsStore {
        fn with_queue(queue: SqsQueue) -> Self {
            Self {
                queue: Mutex::new(Some(queue)),
                sent_messages: Mutex::new(Vec::new()),
            }
        }
    }

    #[async_trait]
    impl SqsStore for TestSqsStore {
        async fn create_queue(&self, _queue: SqsQueue) -> Result<(), StoreError> {
            unimplemented!()
        }

        async fn get_queue(
            &self,
            queue_name: &str,
            region: &str,
            account_id: &str,
        ) -> Result<Option<SqsQueue>, StoreError> {
            Ok(self
                .queue
                .lock()
                .expect("queue mutex")
                .as_ref()
                .filter(|queue| {
                    queue.name == queue_name
                        && queue.region == region
                        && queue.account_id == account_id
                })
                .cloned())
        }

        async fn send_message(&self, message: &SqsMessage) -> Result<(), StoreError> {
            self.sent_messages
                .lock()
                .expect("sent messages mutex")
                .push(message.clone());
            Ok(())
        }
    }

    fn resolved_request(body: &str) -> ResolvedRequest {
        let mut headers = HashMap::new();
        headers.insert(
            "x-amz-target".to_string(),
            "AmazonSQS.SendMessage".to_string(),
        );

        ResolvedRequest {
            request: hiraeth_http::IncomingRequest {
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
                        .with_ymd_and_hms(2026, 4, 2, 12, 0, 0)
                        .unwrap()
                        .naive_utc(),
                },
            },
            date: Utc.with_ymd_and_hms(2026, 4, 2, 12, 0, 0).unwrap(),
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
            receive_message_wait_time_seconds: 0,
        }
    }

    fn parse_json_body(response: &ServiceResponse) -> Value {
        serde_json::from_slice(&response.body).expect("response body should be valid json")
    }

    #[tokio::test]
    async fn send_message_persists_message_and_returns_md5_values() {
        let store = TestSqsStore::with_queue(queue());
        let request = resolved_request(
            r#"{
                "QueueUrl":"http://localhost:4566/123456789012/orders",
                "MessageBody":"hello world",
                "MessageAttributes":{
                    "trace_id":{
                        "DataType":"String",
                        "StringValue":"abc123"
                    }
                }
            }"#,
        );

        let response = send_message(&request, &store)
            .await
            .expect("send message should succeed");

        assert_eq!(response.status_code, 200);

        let response_body = parse_json_body(&response);
        assert_eq!(
            response_body["MD5OfMessageBody"],
            "5eb63bbbe01eeed093cb22bb8f5acdc3"
        );
        assert_eq!(
            response_body["MD5OfMessageAttributes"],
            "853c383c82274bde6eac88d91ee96efe"
        );
        assert!(response_body["MessageId"].as_str().is_some());

        let sent_messages = store.sent_messages.lock().expect("sent messages mutex");
        assert_eq!(sent_messages.len(), 1);
        assert_eq!(sent_messages[0].queue_id, 42);
        assert_eq!(sent_messages[0].body, "hello world");
        assert_eq!(
            sent_messages[0].message_attributes.as_deref(),
            Some(r#"{"trace_id":{"DataType":"String","StringValue":"abc123","BinaryValue":null}}"#)
        );
        assert_eq!(
            sent_messages[0].visible_at,
            Utc.with_ymd_and_hms(2026, 4, 2, 12, 0, 5)
                .unwrap()
                .naive_utc()
        );
    }

    #[tokio::test]
    async fn send_message_uses_request_delay_over_queue_delay() {
        let store = TestSqsStore::with_queue(queue());
        let request = resolved_request(
            r#"{
                "QueueUrl":"http://localhost:4566/123456789012/orders",
                "MessageBody":"hello world",
                "DelaySeconds":12
            }"#,
        );

        let response = send_message(&request, &store)
            .await
            .expect("send message should succeed");

        assert_eq!(response.status_code, 200);
        assert!(parse_json_body(&response)["MD5OfMessageAttributes"].is_null());

        let sent_messages = store.sent_messages.lock().expect("sent messages mutex");
        assert_eq!(sent_messages.len(), 1);
        assert_eq!(
            sent_messages[0].visible_at,
            Utc.with_ymd_and_hms(2026, 4, 2, 12, 0, 12)
                .unwrap()
                .naive_utc()
        );
        assert_eq!(sent_messages[0].message_attributes, None);
    }

    #[tokio::test]
    async fn send_message_returns_queue_not_found_for_unknown_queue() {
        let store = TestSqsStore::default();
        let request = resolved_request(
            r#"{
                "QueueUrl":"http://localhost:4566/123456789012/orders",
                "MessageBody":"hello world"
            }"#,
        );

        let result = send_message(&request, &store).await;

        assert!(matches!(
            result,
            Err(SqsError::QueueNotFound(name, region, account))
                if name == "orders" && region == "us-east-1" && account == "123456789012"
        ));
    }

    #[test]
    fn message_attribute_value_deserializes_pascal_case_fields() {
        let value: MessageAttributeValue = serde_json::from_str(
            r#"{"DataType":"String","StringValue":"abc123","BinaryValue":null}"#,
        )
        .expect("message attribute should deserialize");

        assert_eq!(value.data_type, "String");
        assert_eq!(value.string_value.as_deref(), Some("abc123"));
        assert_eq!(value.binary_value, None);
    }
}
