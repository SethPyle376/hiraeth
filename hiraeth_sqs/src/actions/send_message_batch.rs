use std::collections::HashMap;

use async_trait::async_trait;
use futures::StreamExt;
use hiraeth_core::{
    AwsActionPayloadFormat, AwsActionPayloadParseError, ResolvedRequest, ServiceResponse,
    TypedAwsAction, auth::AuthorizationCheck, json_response,
};
use hiraeth_store::sqs::{SqsMessage, SqsQueue, SqsStore};
use serde::{Deserialize, Serialize};

use super::{
    action_support::{json_payload_format, parse_payload_error},
    send_message::{resolve_delay_seconds, validate_message_body},
};
use crate::{
    error::{SqsError, batch_error_details},
    util::{self, MessageAttributeValue},
};

pub(crate) struct SendMessageBatchAction;

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub(crate) struct SendMessageBatchEntry {
    id: String,
    delay_seconds: Option<i64>,
    #[serde(default)]
    message_attributes: Option<HashMap<String, util::MessageAttributeValue>>,
    #[serde(default)]
    message_system_attributes: Option<HashMap<String, util::MessageAttributeValue>>,
    message_body: String,
    message_deduplication_id: Option<String>,
    message_group_id: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub(crate) struct SendMessageBatchRequest {
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

async fn handle_send_message_batch_typed<S: SqsStore>(
    request: &ResolvedRequest,
    store: &S,
    request_body: SendMessageBatchRequest,
) -> Result<ServiceResponse, SqsError> {
    let queue = crate::util::load_queue_from_url(request, store, &request_body.queue_url).await?;
    crate::util::validate_batch_request(
        request_body.entries.iter().map(|entry| entry.id.as_str()),
    )?;

    let expires_at = request.date.naive_utc()
        + chrono::Duration::seconds(queue.message_retention_period_seconds);

    let messages = futures::stream::iter(request_body.entries)
        .map(|entry| async move {
            let delay_seconds = resolve_delay_seconds(entry.delay_seconds, queue.delay_seconds)
                .map_err(|error| batch_error_entry(&entry.id, error))?;
            validate_message_body(&entry.message_body, queue.maximum_message_size)
                .map_err(|error| batch_error_entry(&entry.id, error))?;

            let visible_at = request.date.naive_utc() + chrono::Duration::seconds(delay_seconds);

            let message_attributes = entry
                .message_attributes
                .as_ref()
                .map(util::serialize_message_attributes)
                .transpose()
                .map_err(|error| batch_error_entry(&entry.id, error))?;

            let md5_of_message_attributes = entry
                .message_attributes
                .as_ref()
                .filter(|attrs| !attrs.is_empty())
                .map(util::calculate_message_attributes_md5)
                .transpose()
                .map_err(|error| batch_error_entry(&entry.id, error))?;

            let aws_trace_header =
                util::extract_aws_trace_header(entry.message_system_attributes.as_ref())
                    .map_err(|error| batch_error_entry(&entry.id, error))?;

            let md5_of_message_system_attributes = entry
                .message_system_attributes
                .as_ref()
                .filter(|attrs| !attrs.is_empty())
                .map(util::calculate_message_attributes_md5)
                .transpose()
                .map_err(|error| batch_error_entry(&entry.id, error))?;

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
                receipt_handle: None,
                first_received_at: None,
                message_group_id: entry.message_group_id.clone(),
                message_deduplication_id: entry.message_deduplication_id.clone(),
            };

            store.send_message(&message).await.map_err(|error| {
                batch_error_entry(&entry.id, SqsError::InternalError(error.to_string()))
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

    json_response(&SendMessageBatchResponse { successful, failed }).map_err(Into::into)
}

fn batch_error_entry(id: &str, error: SqsError) -> BatchResultErrorEntry {
    let (code, sender_fault) = batch_error_details(&error);
    BatchResultErrorEntry {
        id: id.to_string(),
        code: code.to_string(),
        message: error.to_string(),
        sender_fault,
    }
}

#[async_trait]
impl<S> TypedAwsAction<S> for SendMessageBatchAction
where
    S: SqsStore + Send + Sync,
{
    type Request = SendMessageBatchRequest;
    type Error = SqsError;

    fn name(&self) -> &'static str {
        "SendMessageBatch"
    }

    fn payload_format(&self) -> AwsActionPayloadFormat {
        json_payload_format()
    }

    fn parse_error(&self, error: AwsActionPayloadParseError) -> SqsError {
        parse_payload_error(error)
    }

    async fn handle_typed(
        &self,
        request: ResolvedRequest,
        request_body: SendMessageBatchRequest,
        store: &S,
    ) -> Result<ServiceResponse, SqsError> {
        handle_send_message_batch_typed(&request, store, request_body).await
    }

    async fn resolve_authorization_typed(
        &self,
        request: &ResolvedRequest,
        _payload: SendMessageBatchRequest,
        store: &S,
    ) -> Result<AuthorizationCheck, SqsError> {
        crate::auth::resolve_authorization("sqs:SendMessage", request, store).await
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use chrono::{TimeZone, Utc};
    use hiraeth_core::{AuthContext, ResolvedRequest, TypedAwsAction};
    use hiraeth_http::IncomingRequest;
    use hiraeth_router::ServiceResponse;
    use hiraeth_store::{principal::Principal, sqs::SqsQueue, test_support::SqsTestStore};
    use serde_json::Value;

    use super::{SendMessageBatchAction, handle_send_message_batch_typed};

    fn resolved_request(body: &str) -> ResolvedRequest {
        let mut headers = HashMap::new();
        headers.insert(
            "x-amz-target".to_string(),
            "AmazonSQS.SendMessageBatch".to_string(),
        );

        ResolvedRequest {
            request_id: "test-request-id".to_string(),
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
                    path: "/".to_string(),
                    user_id: "AIDATESTUSER000001".to_string(),
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
            created_at: Utc
                .with_ymd_and_hms(2026, 4, 2, 12, 0, 0)
                .unwrap()
                .naive_utc(),
            updated_at: Utc
                .with_ymd_and_hms(2026, 4, 2, 12, 0, 0)
                .unwrap()
                .naive_utc(),
            ..Default::default()
        }
    }

    fn parse_json_body(response: &ServiceResponse) -> Value {
        serde_json::from_slice(&response.body).expect("response body should be valid json")
    }

    #[test]
    fn reports_expected_action_name() {
        assert_eq!(
            <SendMessageBatchAction as TypedAwsAction<SqsTestStore>>::name(&SendMessageBatchAction),
            "SendMessageBatch"
        );
    }

    #[tokio::test]
    async fn returns_sdk_compatible_response_shape() {
        let store = SqsTestStore::with_queue(queue());
        let request = resolved_request(
            r#"{
                "QueueUrl":"http://localhost:4566/123456789012/orders",
                "Entries":[
                    {"Id":"entry-1","MessageBody":"hello world"},
                    {"Id":"entry-2","MessageBody":"goodbye world"}
                ]
            }"#,
        );

        let response = handle_send_message_batch_typed(
            &request,
            &store,
            crate::actions::test_support::parse_request_body(&request),
        )
        .await
        .expect("send message batch should succeed");

        assert_eq!(response.status_code, 200);
        let body = parse_json_body(&response);
        assert_eq!(body["Successful"].as_array().unwrap().len(), 2);
        assert_eq!(body["Failed"].as_array().unwrap().len(), 0);
        assert_eq!(body["Successful"][0]["Id"], "entry-1");
        assert_eq!(body["Successful"][1]["Id"], "entry-2");
    }

    #[tokio::test]
    async fn rejects_duplicate_entry_ids() {
        let store = SqsTestStore::with_queue(queue());
        let request = resolved_request(
            r#"{
                "QueueUrl":"http://localhost:4566/123456789012/orders",
                "Entries":[
                    {"Id":"duplicate","MessageBody":"hello"},
                    {"Id":"duplicate","MessageBody":"goodbye"}
                ]
            }"#,
        );

        let result = handle_send_message_batch_typed(
            &request,
            &store,
            crate::actions::test_support::parse_request_body(&request),
        )
        .await;

        assert!(matches!(
            result,
            Err(crate::error::SqsError::BatchEntryIdsNotDistinct)
        ));
    }
}
