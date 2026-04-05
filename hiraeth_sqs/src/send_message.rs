use std::collections::BTreeMap;

use futures::StreamExt;
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
    #[serde(default)]
    message_system_attributes: Option<BTreeMap<String, util::MessageAttributeValue>>,
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
    #[serde(rename = "MD5OfMessageSystemAttributes")]
    #[serde(skip_serializing_if = "Option::is_none")]
    md5_of_message_system_attributes: Option<String>,
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

    let aws_trace_header =
        util::extract_aws_trace_header(request_body.message_system_attributes.as_ref())?;

    let md5_of_message_system_attributes = request_body
        .message_system_attributes
        .as_ref()
        .filter(|attrs| !attrs.is_empty())
        .map(util::calculate_message_attributes_md5)
        .transpose()?;

    let message = SqsMessage {
        message_id: uuid::Uuid::new_v4().to_string(),
        queue_id: queue.id,
        body: request_body.message_body.clone(),
        message_attributes,
        aws_trace_header,
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
        md5_of_message_system_attributes,
        message_id: message.message_id.clone(),
        sequence_number: None,
    };

    Ok(ServiceResponse {
        status_code: 200,
        headers: vec![],
        body: serde_json::to_vec(&response).unwrap_or_default(),
    })
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct SendMessageBatchEntry {
    id: String,
    delay_seconds: Option<i64>,
    #[serde(default)]
    message_attributes: Option<BTreeMap<String, util::MessageAttributeValue>>,
    #[serde(default)]
    message_system_attributes: Option<BTreeMap<String, util::MessageAttributeValue>>,
    message_body: String,
    message_deduplication_id: Option<String>,
    message_group_id: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct SendMessageBatchRequest {
    entries: Vec<SendMessageBatchEntry>,
    queue_url: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "PascalCase")]
struct SendMessageBatchResultEntry {
    id: String,
    #[serde(rename = "MD5OfMessageAttributes")]
    #[serde(skip_serializing_if = "Option::is_none")]
    md5_of_message_attributes: Option<String>,
    #[serde(rename = "MD5OfMessageBody")]
    md5_of_message_body: String,
    #[serde(rename = "MD5OfMessageSystemAttributes")]
    #[serde(skip_serializing_if = "Option::is_none")]
    md5_of_message_system_attributes: Option<String>,
    message_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    sequence_number: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "PascalCase")]
struct BatchResultErrorEntry {
    id: String,
    code: String,
    message: String,
    sender_fault: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "PascalCase")]
struct SendMessageBatchResponse {
    successful: Vec<SendMessageBatchResultEntry>,
    failed: Vec<BatchResultErrorEntry>,
}

pub(crate) async fn send_message_batch<S: SqsStore>(
    request: &ResolvedRequest,
    store: &S,
) -> Result<ServiceResponse, SqsError> {
    let request_body = serde_json::from_str::<SendMessageBatchRequest>(
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

    let expires_at = request.date.naive_utc()
        + chrono::Duration::seconds(queue.message_retention_period_seconds);

    let messages = futures::stream::iter(request_body.entries)
        .map(|entry| async move {
            let visible_at = request.date.naive_utc()
                + chrono::Duration::seconds(entry.delay_seconds.unwrap_or(queue.delay_seconds));

            let message_attributes = entry
                .message_attributes
                .as_ref()
                .map(|attrs| {
                    serde_json::to_string(attrs).map_err(|e| SqsError::BadRequest(e.to_string()))
                })
                .transpose()
                .map_err(|e| BatchResultErrorEntry {
                    id: entry.id.clone(),
                    code: "InvalidParameterValue".to_string(),
                    message: format!("{:?}", e),
                    sender_fault: true,
                })?;

            let md5_of_message_attributes = entry
                .message_attributes
                .as_ref()
                .filter(|attrs| !attrs.is_empty())
                .map(util::calculate_message_attributes_md5)
                .transpose()
                .map_err(|e| BatchResultErrorEntry {
                    id: entry.id.clone(),
                    code: "InvalidParameterValue".to_string(),
                    message: format!("{:?}", e),
                    sender_fault: true,
                })?;

            let aws_trace_header = util::extract_aws_trace_header(
                entry.message_system_attributes.as_ref(),
            )
            .map_err(|e| BatchResultErrorEntry {
                id: entry.id.clone(),
                code: "InvalidParameterValue".to_string(),
                message: format!("{:?}", e),
                sender_fault: true,
            })?;

            let md5_of_message_system_attributes = entry
                .message_system_attributes
                .as_ref()
                .filter(|attrs| !attrs.is_empty())
                .map(util::calculate_message_attributes_md5)
                .transpose()
                .map_err(|e| BatchResultErrorEntry {
                    id: entry.id.clone(),
                    code: "InvalidParameterValue".to_string(),
                    message: format!("{:?}", e),
                    sender_fault: true,
                })?;

        let message = SqsMessage {
            message_id: uuid::Uuid::new_v4().to_string(),
            queue_id: queue.id,
            body: entry.message_body.clone(),
            message_attributes,
            aws_trace_header,
                sent_at: request.date.naive_utc(),
                visible_at,
                expires_at,
                receive_count: 0,
                receipt_handle: Option::None,
                first_received_at: Option::None,
                message_group_id: entry.message_group_id.clone(),
                message_deduplication_id: entry.message_deduplication_id.clone(),
            };

            store
                .send_message(&message)
                .await
                .map_err(|e| BatchResultErrorEntry {
                    id: entry.id.clone(),
                    code: "InternalError".to_string(),
                    message: format!("{:?}", e),
                    sender_fault: false,
                })?;

            Ok(SendMessageBatchResultEntry {
                id: entry.id.clone(),
                md5_of_message_attributes,
                md5_of_message_body: format!("{:x}", md5::compute(entry.message_body.as_bytes())),
                md5_of_message_system_attributes,
                message_id: message.message_id.clone(),
                sequence_number: None,
            })
        })
        .buffer_unordered(1)
        .collect::<Vec<Result<SendMessageBatchResultEntry, BatchResultErrorEntry>>>()
        .await;

    let successful = messages
        .iter()
        .filter_map(|result| result.as_ref().ok().cloned())
        .collect();

    let failed = messages
        .iter()
        .filter_map(|result| result.as_ref().err().cloned())
        .collect();

    return Ok(ServiceResponse {
        status_code: 200,
        headers: vec![],
        body: serde_json::to_vec(&SendMessageBatchResponse { successful, failed })
            .unwrap_or_default(),
    });
}

#[cfg(test)]
mod tests {
    use std::{collections::{BTreeMap, HashMap}, sync::Mutex};

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

    use super::{send_message, send_message_batch};
    use crate::{SqsError, util::{self, MessageAttributeValue}};

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

        async fn delete_queue(&self, _queue_id: i64) -> Result<(), StoreError> {
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

        async fn get_message_count(&self, _queue_id: i64) -> Result<i64, StoreError> {
            unimplemented!()
        }

        async fn get_visible_message_count(&self, _queue_id: i64) -> Result<i64, StoreError> {
            unimplemented!()
        }

        async fn get_messages_delayed_count(&self, _queue_id: i64) -> Result<i64, StoreError> {
            unimplemented!()
        }

        async fn send_message(&self, message: &SqsMessage) -> Result<(), StoreError> {
            self.sent_messages
                .lock()
                .expect("sent messages mutex")
                .push(message.clone());
            Ok(())
        }

        async fn receive_messages(
            &self,
            _queue_id: i64,
            _max_number_of_messages: i64,
            _visibility_timeout_seconds: u32,
        ) -> Result<Vec<SqsMessage>, StoreError> {
            unimplemented!()
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

    fn batch_resolved_request(body: &str) -> ResolvedRequest {
        let mut request = resolved_request(body);
        request.request.headers.insert(
            "x-amz-target".to_string(),
            "AmazonSQS.SendMessageBatch".to_string(),
        );
        request
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
            created_at: Utc
                .with_ymd_and_hms(2026, 4, 2, 12, 0, 0)
                .unwrap()
                .naive_utc(),
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
        assert!(response_body["MD5OfMessageSystemAttributes"].is_null());

        let sent_messages = store.sent_messages.lock().expect("sent messages mutex");
        assert_eq!(sent_messages.len(), 1);
        assert_eq!(sent_messages[0].queue_id, 42);
        assert_eq!(sent_messages[0].body, "hello world");
        assert_eq!(
            sent_messages[0].message_attributes.as_deref(),
            Some(r#"{"trace_id":{"DataType":"String","StringValue":"abc123","BinaryValue":null}}"#)
        );
        assert_eq!(sent_messages[0].aws_trace_header, None);
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
    async fn send_message_persists_aws_trace_header_and_returns_system_md5() {
        let store = TestSqsStore::with_queue(queue());
        let request = resolved_request(
            r#"{
                "QueueUrl":"http://localhost:4566/123456789012/orders",
                "MessageBody":"hello world",
                "MessageSystemAttributes":{
                    "AWSTraceHeader":{
                        "DataType":"String",
                        "StringValue":"Root=1-abcdef12-0123456789abcdef01234567"
                    }
                }
            }"#,
        );

        let response = send_message(&request, &store)
            .await
            .expect("send message should succeed");

        let response_body = parse_json_body(&response);
        let expected_md5 = util::calculate_message_attributes_md5(&BTreeMap::from([(
            "AWSTraceHeader".to_string(),
            MessageAttributeValue {
                data_type: "String".to_string(),
                string_value: Some("Root=1-abcdef12-0123456789abcdef01234567".to_string()),
                binary_value: None,
            },
        )]))
        .expect("system attributes should hash");
        assert_eq!(
            response_body["MD5OfMessageSystemAttributes"],
            expected_md5
        );

        let sent_messages = store.sent_messages.lock().expect("sent messages mutex");
        assert_eq!(
            sent_messages[0].aws_trace_header.as_deref(),
            Some("Root=1-abcdef12-0123456789abcdef01234567")
        );
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

    #[tokio::test]
    async fn send_message_batch_returns_sdk_compatible_response_shape() {
        let store = TestSqsStore::with_queue(queue());
        let request = batch_resolved_request(
            r#"{
                "QueueUrl":"http://localhost:4566/123456789012/orders",
                "Entries":[
                    {
                        "Id":"first",
                        "MessageBody":"hello world"
                    },
                    {
                        "Id":"second",
                        "MessageBody":"goodbye world",
                        "MessageAttributes":{
                            "trace_id":{
                                "DataType":"String",
                                "StringValue":"abc123"
                            }
                        }
                    }
                ]
            }"#,
        );

        let response = send_message_batch(&request, &store)
            .await
            .expect("send message batch should succeed");

        assert_eq!(response.status_code, 200);

        let response_body = parse_json_body(&response);
        let successful = response_body["Successful"]
            .as_array()
            .expect("Successful should be an array");
        let failed = response_body["Failed"]
            .as_array()
            .expect("Failed should be an array");

        assert_eq!(successful.len(), 2);
        assert!(failed.is_empty());

        assert_eq!(successful[0]["Id"], "first");
        assert_eq!(
            successful[0]["MD5OfMessageBody"],
            "5eb63bbbe01eeed093cb22bb8f5acdc3"
        );
        assert!(successful[0]["MessageId"].as_str().is_some());
        assert!(successful[0].get("Success").is_none());
        assert!(successful[0].get("SequenceNumber").is_none());

        assert_eq!(successful[1]["Id"], "second");
        assert_eq!(
            successful[1]["MD5OfMessageAttributes"],
            "853c383c82274bde6eac88d91ee96efe"
        );
        assert!(successful[1].get("Success").is_none());
    }
}
