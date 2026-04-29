use std::collections::HashMap;

use async_trait::async_trait;
use hiraeth_core::{
    AwsActionPayloadFormat, AwsActionPayloadParseError, ResolvedRequest, ServiceResponse,
    TypedAwsAction,
    auth::AuthorizationCheck,
    json_response,
    tracing::{TraceContext, TraceRecorder},
};
use hiraeth_store::sqs::{SqsMessage, SqsQueue, SqsStore};
use serde::{Deserialize, Serialize};

use super::action_support::{json_payload_format, parse_payload_error};
use crate::{
    error::SqsError,
    util::{self, MessageAttributeValue},
};

pub(crate) struct SendMessageAction;

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub(crate) struct SendMessageRequest {
    delay_seconds: Option<i64>,
    #[serde(default)]
    message_attributes: Option<HashMap<String, util::MessageAttributeValue>>,
    #[serde(default)]
    message_system_attributes: Option<HashMap<String, util::MessageAttributeValue>>,
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

async fn handle_send_message_typed<S: SqsStore>(
    request: &ResolvedRequest,
    store: &S,
    request_body: SendMessageRequest,
) -> Result<ServiceResponse, SqsError> {
    let queue = crate::util::load_queue_from_url(request, store, &request_body.queue_url).await?;
    validate_message_body(&request_body.message_body, queue.maximum_message_size)?;
    let delay_seconds = resolve_delay_seconds(request_body.delay_seconds, queue.delay_seconds)?;

    let visible_at = request.date.naive_utc() + chrono::Duration::seconds(delay_seconds);
    let expires_at = request.date.naive_utc()
        + chrono::Duration::seconds(queue.message_retention_period_seconds);

    let message_attributes = request_body
        .message_attributes
        .as_ref()
        .map(util::serialize_message_attributes)
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
        receipt_handle: None,
        first_received_at: None,
        message_group_id: request_body.message_group_id.clone(),
        message_deduplication_id: request_body.message_deduplication_id.clone(),
    };

    store
        .send_message(&message)
        .await
        .map_err(|e| SqsError::InternalError(e.to_string()))?;

    let response = SendMessageResponse {
        md5_of_message_attributes,
        md5_of_message_body: format!("{:x}", md5::compute(request_body.message_body.as_bytes())),
        md5_of_message_system_attributes,
        message_id: message.message_id.clone(),
        sequence_number: None,
    };

    json_response(&response).map_err(Into::into)
}

pub(super) fn resolve_delay_seconds(
    requested_delay_seconds: Option<i64>,
    queue_delay_seconds: i64,
) -> Result<i64, SqsError> {
    let delay_seconds = requested_delay_seconds.unwrap_or(queue_delay_seconds);
    if !(0..=900).contains(&delay_seconds) {
        return Err(SqsError::BadRequest(
            "DelaySeconds must be between 0 and 900".to_string(),
        ));
    }

    Ok(delay_seconds)
}

pub(super) fn validate_message_body(
    message_body: &str,
    maximum_message_size: i64,
) -> Result<(), SqsError> {
    if message_body.len() > maximum_message_size as usize {
        return Err(SqsError::BadRequest(format!(
            "MessageBody exceeds the queue MaximumMessageSize of {} bytes",
            maximum_message_size
        )));
    }

    Ok(())
}

#[async_trait]
impl<S> TypedAwsAction<S> for SendMessageAction
where
    S: SqsStore + Send + Sync,
{
    type Request = SendMessageRequest;
    type Error = SqsError;

    fn name(&self) -> &'static str {
        "SendMessage"
    }

    fn payload_format(&self) -> AwsActionPayloadFormat {
        json_payload_format()
    }

    fn parse_error(&self, error: AwsActionPayloadParseError) -> SqsError {
        parse_payload_error(error)
    }

    async fn handle(
        &self,
        request: ResolvedRequest,
        request_body: SendMessageRequest,
        store: &S,
        trace_context: &TraceContext,
        trace_recorder: &dyn TraceRecorder,
    ) -> Result<ServiceResponse, SqsError> {
        let timer = trace_context.start_span();
        let attributes = HashMap::from([
            ("queue_url".to_string(), request_body.queue_url.clone()),
            (
                "body_bytes".to_string(),
                request_body.message_body.len().to_string(),
            ),
            (
                "delay_seconds".to_string(),
                request_body
                    .delay_seconds
                    .map(|delay_seconds| delay_seconds.to_string())
                    .unwrap_or_else(|| "queue_default".to_string()),
            ),
            (
                "message_attribute_count".to_string(),
                request_body
                    .message_attributes
                    .as_ref()
                    .map(HashMap::len)
                    .unwrap_or_default()
                    .to_string(),
            ),
            (
                "system_attribute_count".to_string(),
                request_body
                    .message_system_attributes
                    .as_ref()
                    .map(HashMap::len)
                    .unwrap_or_default()
                    .to_string(),
            ),
            (
                "has_message_group_id".to_string(),
                request_body.message_group_id.is_some().to_string(),
            ),
            (
                "has_message_deduplication_id".to_string(),
                request_body.message_deduplication_id.is_some().to_string(),
            ),
        ]);

        let result = handle_send_message_typed(&request, store, request_body).await;
        let status = if result.is_ok() { "ok" } else { "error" };
        trace_context
            .record_span_or_warn(
                trace_recorder,
                timer,
                "sqs.send_message.persist",
                "sqs",
                status,
                attributes,
            )
            .await;

        result
    }

    async fn resolve_authorization_typed(
        &self,
        request: &ResolvedRequest,
        _payload: SendMessageRequest,
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

    use super::{MessageAttributeValue, SendMessageAction, handle_send_message_typed};
    use crate::{error::SqsError, util};

    fn resolved_request(body: &str) -> ResolvedRequest {
        let mut headers = HashMap::new();
        headers.insert(
            "x-amz-target".to_string(),
            "AmazonSQS.SendMessage".to_string(),
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
            <SendMessageAction as TypedAwsAction<SqsTestStore>>::name(&SendMessageAction),
            "SendMessage"
        );
    }

    #[tokio::test]
    async fn persists_message_and_returns_md5_values() {
        let store = SqsTestStore::with_queue(queue());
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

        let response = handle_send_message_typed(
            &request,
            &store,
            crate::actions::test_support::parse_request_body(&request),
        )
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

        let sent_messages = store.sent_messages();
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
    async fn uses_request_delay_over_queue_delay() {
        let store = SqsTestStore::with_queue(queue());
        let request = resolved_request(
            r#"{
                "QueueUrl":"http://localhost:4566/123456789012/orders",
                "MessageBody":"hello world",
                "DelaySeconds":12
            }"#,
        );

        let response = handle_send_message_typed(
            &request,
            &store,
            crate::actions::test_support::parse_request_body(&request),
        )
        .await
        .expect("send message should succeed");

        assert_eq!(response.status_code, 200);
        assert!(parse_json_body(&response)["MD5OfMessageAttributes"].is_null());

        let sent_messages = store.sent_messages();
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
    async fn persists_aws_trace_header_and_returns_system_md5() {
        let store = SqsTestStore::with_queue(queue());
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

        let response = handle_send_message_typed(
            &request,
            &store,
            crate::actions::test_support::parse_request_body(&request),
        )
        .await
        .expect("send message should succeed");

        let response_body = parse_json_body(&response);
        let expected_md5 = util::calculate_message_attributes_md5(&HashMap::from([(
            "AWSTraceHeader".to_string(),
            MessageAttributeValue {
                data_type: "String".to_string(),
                string_value: Some("Root=1-abcdef12-0123456789abcdef01234567".to_string()),
                binary_value: None,
            },
        )]))
        .expect("system attributes should hash");
        assert_eq!(response_body["MD5OfMessageSystemAttributes"], expected_md5);

        let sent_messages = store.sent_messages();
        assert_eq!(
            sent_messages[0].aws_trace_header.as_deref(),
            Some("Root=1-abcdef12-0123456789abcdef01234567")
        );
    }

    #[tokio::test]
    async fn returns_queue_not_found_for_unknown_queue() {
        let store = SqsTestStore::default();
        let request = resolved_request(
            r#"{
                "QueueUrl":"http://localhost:4566/123456789012/orders",
                "MessageBody":"hello world"
            }"#,
        );

        let result = handle_send_message_typed(
            &request,
            &store,
            crate::actions::test_support::parse_request_body(&request),
        )
        .await;

        assert!(matches!(result, Err(SqsError::QueueNotFound)));
    }

    #[test]
    fn message_attribute_value_deserializes_pascal_case_fields() {
        let value: MessageAttributeValue = serde_json::from_str(
            r#"{"DataType":"String","StringValue":"abc123","BinaryValue":null}"#,
        )
        .expect("message attribute should deserialize");

        assert_eq!(value.data_type, "String");
        assert_eq!(value.string_value.as_deref(), Some("abc123"));
        assert!(value.binary_value.is_none());
    }
}
