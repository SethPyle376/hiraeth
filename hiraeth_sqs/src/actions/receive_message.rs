use std::{cmp::min, collections::BTreeMap};

use async_trait::async_trait;
use chrono::Utc;
use hiraeth_core::{
    AwsActionPayloadFormat, AwsActionPayloadParseError, ResolvedRequest, ServiceResponse,
    TypedAwsAction, auth::AuthorizationCheck, json_response,
};
use hiraeth_store::sqs::{SqsMessage, SqsQueue, SqsStore};
use serde::{Deserialize, Serialize};

use super::action_support::{json_payload_format, parse_payload_error};
use crate::{error::SqsError, util};

pub(crate) struct ReceiveMessageAction;

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

async fn handle_receive_message_typed<S: SqsStore>(
    request: &ResolvedRequest,
    store: &S,
    receive_request: ReceiveMessageRequest,
) -> Result<ServiceResponse, SqsError> {
    let queue =
        crate::util::load_queue_from_url(request, store, &receive_request.queue_url).await?;
    validate_receive_request(&receive_request)?;

    let visibility_timeout_seconds = receive_request
        .visibility_timeout
        .unwrap_or(queue.visibility_timeout_seconds as u32);
    let wait_time_seconds = receive_request
        .wait_time_seconds
        .unwrap_or(queue.receive_message_wait_time_seconds as u32);
    let deadline = Utc::now() + chrono::Duration::seconds(wait_time_seconds as i64);

    let received_messages = loop {
        let messages = store
            .receive_messages(
                queue.id,
                receive_request.max_number_of_messages,
                visibility_timeout_seconds,
            )
            .await
            .map_err(|e| SqsError::InternalError(e.to_string()))?;

        let received_messages = messages
            .into_iter()
            .map(|message| {
                let attributes = select_system_attributes(&receive_request, &message);
                let message_attributes = filter_message_attributes(
                    parse_message_attributes(&message)?,
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
                    message_id: message.message_id,
                    receipt_handle: message.receipt_handle.unwrap_or_default(),
                    body: message.body.clone(),
                    md5_of_body: Some(format!("{:x}", md5::compute(message.body.as_bytes()))),
                    md5_of_message_attributes,
                })
            })
            .collect::<Result<Vec<_>, SqsError>>()?;

        if !received_messages.is_empty() {
            break received_messages;
        }

        if Utc::now() >= deadline {
            break Vec::new();
        }

        let sleep_until = min(deadline, Utc::now() + chrono::Duration::milliseconds(100));
        tokio::time::sleep(
            sleep_until
                .signed_duration_since(Utc::now())
                .to_std()
                .unwrap_or_default(),
        )
        .await;
    };

    json_response(&ReceiveMessageResponse {
        messages: received_messages,
    })
    .map_err(Into::into)
}

fn validate_receive_request(request: &ReceiveMessageRequest) -> Result<(), SqsError> {
    if !(1..=10).contains(&request.max_number_of_messages) {
        return Err(SqsError::BadRequest(
            "MaxNumberOfMessages must be between 1 and 10".to_string(),
        ));
    }

    if request
        .visibility_timeout
        .is_some_and(|visibility_timeout| visibility_timeout > 43200)
    {
        return Err(SqsError::BadRequest(
            "VisibilityTimeout must be between 0 and 43200".to_string(),
        ));
    }

    if request
        .wait_time_seconds
        .is_some_and(|wait_time_seconds| wait_time_seconds > 20)
    {
        return Err(SqsError::BadRequest(
            "WaitTimeSeconds must be between 0 and 20".to_string(),
        ));
    }

    Ok(())
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
    message: &SqsMessage,
) -> BTreeMap<String, String> {
    let mut attributes = BTreeMap::new();

    if should_include_system_attribute("ApproximateReceiveCount", request) {
        attributes.insert(
            "ApproximateReceiveCount".to_string(),
            message.receive_count.to_string(),
        );
    }
    if should_include_system_attribute("ApproximateFirstReceiveTimestamp", request)
        && let Some(first_received_at) = message.first_received_at
    {
        attributes.insert(
            "ApproximateFirstReceiveTimestamp".to_string(),
            epoch_millis(first_received_at),
        );
    }
    if should_include_system_attribute("SentTimestamp", request) {
        attributes.insert("SentTimestamp".to_string(), epoch_millis(message.sent_at));
    }
    if should_include_system_attribute("AWSTraceHeader", request)
        && let Some(aws_trace_header) = &message.aws_trace_header
    {
        attributes.insert("AWSTraceHeader".to_string(), aws_trace_header.clone());
    }
    if should_include_system_attribute("MessageDeduplicationId", request)
        && let Some(message_deduplication_id) = &message.message_deduplication_id
    {
        attributes.insert(
            "MessageDeduplicationId".to_string(),
            message_deduplication_id.clone(),
        );
    }
    if should_include_system_attribute("MessageGroupId", request)
        && let Some(message_group_id) = &message.message_group_id
    {
        attributes.insert("MessageGroupId".to_string(), message_group_id.clone());
    }

    attributes
}

fn parse_message_attributes(
    message: &SqsMessage,
) -> Result<BTreeMap<String, util::MessageAttributeValue>, SqsError> {
    match message.message_attributes.as_deref() {
        Some(raw) if !raw.is_empty() => serde_json::from_str(raw).map_err(|e| {
            SqsError::InternalError(format!(
                "failed to parse stored message attributes for message {}: {}",
                message.message_id, e
            ))
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

#[async_trait]
impl<S> TypedAwsAction<S> for ReceiveMessageAction
where
    S: SqsStore + Send + Sync,
{
    type Request = ReceiveMessageRequest;
    type Error = SqsError;

    fn name(&self) -> &'static str {
        "ReceiveMessage"
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
        receive_request: ReceiveMessageRequest,
        store: &S,
    ) -> Result<ServiceResponse, SqsError> {
        handle_receive_message_typed(&request, store, receive_request).await
    }

    async fn resolve_authorization_typed(
        &self,
        request: &ResolvedRequest,
        _payload: ReceiveMessageRequest,
        store: &S,
    ) -> Result<AuthorizationCheck, SqsError> {
        crate::auth::resolve_authorization("sqs:ReceiveMessage", request, store).await
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use chrono::{TimeZone, Utc};
    use hiraeth_core::{AuthContext, ResolvedRequest, TypedAwsAction};
    use hiraeth_http::IncomingRequest;
    use hiraeth_router::ServiceResponse;
    use hiraeth_store::{
        principal::Principal,
        sqs::{SqsMessage, SqsQueue},
        test_support::SqsTestStore,
    };
    use serde_json::Value;

    use super::{ReceiveMessageAction, handle_receive_message_typed};
    use crate::error::SqsError;

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
            updated_at: Utc
                .with_ymd_and_hms(2026, 4, 3, 12, 0, 0)
                .unwrap()
                .naive_utc(),
            ..Default::default()
        }
    }

    fn resolved_request(body: &str) -> ResolvedRequest {
        let mut headers = HashMap::new();
        headers.insert(
            "x-amz-target".to_string(),
            "AmazonSQS.ReceiveMessage".to_string(),
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

    #[test]
    fn reports_expected_action_name() {
        assert_eq!(
            <ReceiveMessageAction as TypedAwsAction<SqsTestStore>>::name(&ReceiveMessageAction),
            "ReceiveMessage"
        );
    }

    #[tokio::test]
    async fn returns_requested_message_and_system_attributes() {
        let store = SqsTestStore::with_queue(queue()).with_receive_responses([vec![message(42)]]);
        let request = resolved_request(
            r#"{
                "QueueUrl":"http://localhost:4566/123456789012/orders",
                "MessageAttributeNames":["All"],
                "MessageSystemAttributeNames":["ApproximateReceiveCount","ApproximateFirstReceiveTimestamp","SentTimestamp","MessageGroupId","MessageDeduplicationId","AWSTraceHeader"]
            }"#,
        );

        let response = handle_receive_message_typed(
            &request,
            &store,
            crate::actions::test_support::parse_request_body(&request),
        )
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
    async fn filters_requested_message_attributes() {
        let store = SqsTestStore::with_queue(queue()).with_receive_responses([vec![message(42)]]);
        let request = resolved_request(
            r#"{
                "QueueUrl":"http://localhost:4566/123456789012/orders",
                "AttributeNames":["SentTimestamp"],
                "MessageAttributeNames":["trace_id"]
            }"#,
        );

        let response = handle_receive_message_typed(
            &request,
            &store,
            crate::actions::test_support::parse_request_body(&request),
        )
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
    async fn returns_queue_not_found_for_missing_queue() {
        let mut missing_queue = queue();
        missing_queue.name = "other".to_string();
        let store =
            SqsTestStore::with_queue(missing_queue).with_receive_responses([vec![message(42)]]);
        let request =
            resolved_request(r#"{"QueueUrl":"http://localhost:4566/123456789012/orders"}"#);

        let result = handle_receive_message_typed(
            &request,
            &store,
            crate::actions::test_support::parse_request_body(&request),
        )
        .await;

        assert!(matches!(result, Err(SqsError::QueueNotFound)));
    }

    #[tokio::test]
    async fn rejects_invalid_max_number_of_messages() {
        let store = SqsTestStore::with_queue(queue());
        let request = resolved_request(
            r#"{
                "QueueUrl":"http://localhost:4566/123456789012/orders",
                "MaxNumberOfMessages":11
            }"#,
        );

        let result = handle_receive_message_typed(
            &request,
            &store,
            crate::actions::test_support::parse_request_body(&request),
        )
        .await;

        assert!(matches!(result, Err(SqsError::BadRequest(_))));
    }

    #[tokio::test]
    async fn retries_when_wait_time_seconds_is_set() {
        let store = SqsTestStore::with_queue(queue())
            .with_receive_responses([Vec::new(), vec![message(42)]]);
        let request = resolved_request(
            r#"{
                "QueueUrl":"http://localhost:4566/123456789012/orders",
                "WaitTimeSeconds":1
            }"#,
        );

        let response = handle_receive_message_typed(
            &request,
            &store,
            crate::actions::test_support::parse_request_body(&request),
        )
        .await
        .expect("receive message should succeed");

        let body = parse_json_body(&response);
        let messages = body["Messages"]
            .as_array()
            .expect("Messages should be an array");
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0]["MessageId"], "msg-123");
    }
}
