use std::collections::{BTreeMap, HashSet};

use base64::Engine;
use hiraeth_core::{AuthContext, ResolvedRequest, parse_json_body};
use hiraeth_store::sqs::{SqsQueue, SqsStore};
use serde::{Deserialize, Serialize, de::DeserializeOwned};

use crate::error::SqsError;

pub struct QueueId {
    pub name: String,
    pub region: String,
    pub account_id: String,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "PascalCase")]
pub struct MessageAttributeValue {
    pub data_type: String,
    pub string_value: Option<String>,
    pub binary_value: Option<String>,
}

pub fn parse_queue_url(queue_url: &str, default_region: &str) -> Option<QueueId> {
    let url = url::Url::parse(queue_url).ok()?;
    let path_segments: Vec<&str> = url.path_segments()?.collect();
    if path_segments.len() != 2 {
        return None;
    }

    let host = url.host_str()?;
    let region = host
        .strip_prefix("sqs.")
        .and_then(|remainder| remainder.split('.').next())
        .unwrap_or(default_region);

    Some(QueueId {
        name: path_segments[1].to_string(),
        region: region.to_string(),
        account_id: path_segments[0].to_string(),
    })
}

pub(crate) fn parse_request_body<T: DeserializeOwned>(
    request: &ResolvedRequest,
) -> Result<T, SqsError> {
    parse_json_body(&request.request.body).map_err(Into::into)
}

pub(crate) async fn load_queue_from_url<S: SqsStore>(
    request: &ResolvedRequest,
    store: &S,
    queue_url: &str,
) -> Result<SqsQueue, SqsError> {
    let queue_id = parse_queue_url(queue_url, &request.region)
        .ok_or_else(|| SqsError::BadRequest("Invalid queue url".to_string()))?;

    store
        .get_queue(&queue_id.name, &queue_id.region, &queue_id.account_id)
        .await
        .map_err(|e| SqsError::InternalError(e.to_string()))?
        .ok_or(SqsError::QueueNotFound)
}

pub(crate) fn queue_url(host: &str, account_id: &str, queue_name: &str) -> String {
    format!("http://{host}/{account_id}/{queue_name}")
}

pub(crate) fn validate_batch_request<'a>(
    entry_ids: impl IntoIterator<Item = &'a str>,
) -> Result<(), SqsError> {
    let mut count = 0;
    let mut seen = HashSet::new();

    for entry_id in entry_ids {
        count += 1;
        if !seen.insert(entry_id) {
            return Err(SqsError::BatchEntryIdsNotDistinct);
        }
    }

    if count == 0 {
        return Err(SqsError::EmptyBatchRequest);
    }

    if count > 10 {
        return Err(SqsError::TooManyEntriesInBatchRequest);
    }

    Ok(())
}

pub(crate) fn validate_batch_entry_id(entry_id: &str) -> Result<(), SqsError> {
    if entry_id.is_empty() || entry_id.len() > 80 {
        return Err(SqsError::BadRequest(
            "Batch entry Id must be between 1 and 80 characters".to_string(),
        ));
    }

    if !entry_id
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || ch == '-' || ch == '_')
    {
        return Err(SqsError::BadRequest(
            "Batch entry Id may only contain alphanumeric characters, hyphens, and underscores"
                .to_string(),
        ));
    }

    Ok(())
}

pub(crate) fn validate_message_attributes<'a>(
    message_attributes: impl IntoIterator<Item = (&'a String, &'a MessageAttributeValue)>,
) -> Result<(), SqsError> {
    let message_attributes = message_attributes.into_iter().collect::<Vec<_>>();
    if message_attributes.len() > 10 {
        return Err(SqsError::BadRequest(
            "A message can contain at most 10 message attributes".to_string(),
        ));
    }

    for (name, value) in message_attributes {
        validate_message_attribute_name(name)?;
        validate_message_attribute_value(name, value)?;
    }

    Ok(())
}

pub(crate) fn validate_message_system_attributes<'a>(
    message_system_attributes: impl IntoIterator<Item = (&'a String, &'a MessageAttributeValue)>,
) -> Result<(), SqsError> {
    for (name, value) in message_system_attributes {
        if name != "AWSTraceHeader" {
            return Err(SqsError::BadRequest(format!(
                "Unsupported message system attribute: {name}"
            )));
        }
        if value.data_type != "String" {
            return Err(SqsError::BadRequest(
                "AWSTraceHeader must use DataType=String".to_string(),
            ));
        }
        validate_message_attribute_value(name, value)?;
    }

    Ok(())
}

fn validate_message_attribute_name(name: &str) -> Result<(), SqsError> {
    if name.is_empty() || name.len() > 256 {
        return Err(SqsError::BadRequest(
            "Message attribute names must be between 1 and 256 characters".to_string(),
        ));
    }

    if name.starts_with('.') || name.ends_with('.') || name.contains("..") {
        return Err(SqsError::BadRequest(
            "Message attribute names may not start or end with a period or contain consecutive periods"
                .to_string(),
        ));
    }

    if name
        .get(..4)
        .is_some_and(|prefix| prefix.eq_ignore_ascii_case("aws."))
    {
        return Err(SqsError::BadRequest(
            "Message attribute names cannot start with the reserved AWS. prefix".to_string(),
        ));
    }

    if !name
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || ch == '_' || ch == '-' || ch == '.')
    {
        return Err(SqsError::BadRequest(
            "Message attribute names may only contain alphanumeric characters, hyphens, underscores, and periods"
                .to_string(),
        ));
    }

    Ok(())
}

fn validate_message_attribute_value(
    name: &str,
    value: &MessageAttributeValue,
) -> Result<(), SqsError> {
    if value.data_type.is_empty() || value.data_type.len() > 256 {
        return Err(SqsError::BadRequest(format!(
            "Message attribute '{name}' DataType must be between 1 and 256 characters"
        )));
    }

    if value.data_type.starts_with("Binary") {
        if value.binary_value.is_none() {
            return Err(SqsError::BadRequest(format!(
                "Binary message attribute '{name}' is missing BinaryValue"
            )));
        }
        return Ok(());
    }

    if value.data_type.starts_with("String") || value.data_type.starts_with("Number") {
        if value.string_value.is_none() {
            return Err(SqsError::BadRequest(format!(
                "String/Number message attribute '{name}' is missing StringValue"
            )));
        }
        return Ok(());
    }

    Err(SqsError::BadRequest(format!(
        "Message attribute '{name}' DataType must start with String, Number, or Binary"
    )))
}

pub fn calculate_message_attributes_md5<'a>(
    message_attributes: impl IntoIterator<Item = (&'a String, &'a MessageAttributeValue)>,
) -> Result<String, SqsError> {
    let mut buffer = Vec::new();
    let mut message_attributes = message_attributes.into_iter().collect::<Vec<_>>();
    message_attributes.sort_by_key(|(name, _)| *name);

    for (name, value) in message_attributes {
        append_length_prefixed_bytes(&mut buffer, name.as_bytes());
        append_length_prefixed_bytes(&mut buffer, value.data_type.as_bytes());

        if value.data_type.starts_with("Binary") {
            buffer.push(2);
            let binary_value = value.binary_value.as_ref().ok_or_else(|| {
                SqsError::BadRequest(format!(
                    "Binary message attribute '{}' is missing BinaryValue",
                    name
                ))
            })?;
            let decoded = base64::engine::general_purpose::STANDARD
                .decode(binary_value)
                .map_err(|e| {
                    SqsError::BadRequest(format!(
                        "Binary message attribute '{}' is not valid base64: {}",
                        name, e
                    ))
                })?;
            append_length_prefixed_bytes(&mut buffer, &decoded);
        } else {
            buffer.push(1);
            let string_value = value.string_value.as_ref().ok_or_else(|| {
                SqsError::BadRequest(format!(
                    "String/Number message attribute '{}' is missing StringValue",
                    name
                ))
            })?;
            append_length_prefixed_bytes(&mut buffer, string_value.as_bytes());
        }
    }

    Ok(format!("{:x}", md5::compute(buffer)))
}

pub fn serialize_message_attributes<'a>(
    message_attributes: impl IntoIterator<Item = (&'a String, &'a MessageAttributeValue)>,
) -> Result<String, SqsError> {
    let ordered_attributes = message_attributes
        .into_iter()
        .map(|(name, value)| (name.as_str(), value))
        .collect::<BTreeMap<_, _>>();

    serde_json::to_string(&ordered_attributes).map_err(|e| {
        SqsError::InternalError(format!("failed to serialize message attributes: {}", e))
    })
}

pub fn extract_aws_trace_header<'a>(
    message_system_attributes: Option<
        impl IntoIterator<Item = (&'a String, &'a MessageAttributeValue)>,
    >,
) -> Result<Option<String>, SqsError> {
    let Some(message_system_attributes) = message_system_attributes else {
        return Ok(None);
    };

    let Some((_, trace_header)) = message_system_attributes
        .into_iter()
        .find(|(name, _)| name.as_str() == "AWSTraceHeader")
    else {
        return Ok(None);
    };

    if trace_header.data_type != "String" {
        return Err(SqsError::BadRequest(
            "AWSTraceHeader must use DataType=String".to_string(),
        ));
    }

    trace_header
        .string_value
        .clone()
        .ok_or_else(|| SqsError::BadRequest("AWSTraceHeader is missing StringValue".to_string()))
        .map(Some)
}

fn append_length_prefixed_bytes(buffer: &mut Vec<u8>, bytes: &[u8]) {
    buffer.extend_from_slice(&(bytes.len() as u32).to_be_bytes());
    buffer.extend_from_slice(bytes);
}

pub fn resolve_delay_seconds(
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

pub fn validate_message_body(
    message_body: &str,
    maximum_message_size: i64,
) -> Result<(), SqsError> {
    if message_body.is_empty() {
        return Err(SqsError::BadRequest(
            "MessageBody must contain at least one character".to_string(),
        ));
    }

    if message_body.len() > maximum_message_size as usize {
        return Err(SqsError::BadRequest(format!(
            "MessageBody exceeds the queue MaximumMessageSize of {} bytes",
            maximum_message_size
        )));
    }

    if !message_body.chars().all(is_valid_sqs_message_character) {
        return Err(SqsError::BadRequest(
            "MessageBody contains characters that are not allowed by SQS".to_string(),
        ));
    }

    Ok(())
}

fn is_valid_sqs_message_character(ch: char) -> bool {
    matches!(
        ch,
        '\u{9}' | '\u{A}' | '\u{D}' | '\u{20}'..='\u{D7FF}' | '\u{E000}'..='\u{FFFD}' | '\u{10000}'..='\u{10FFFF}'
    )
}

pub fn get_queue_arn(queue: &SqsQueue) -> String {
    format!(
        "arn:aws:sqs:{}:{}:{}",
        queue.region, queue.account_id, queue.name
    )
}

#[cfg(test)]
mod tests {
    use std::collections::{BTreeMap, HashMap};

    use chrono::{TimeZone, Utc};
    use hiraeth_core::{AuthContext, ResolvedRequest};
    use hiraeth_http::IncomingRequest;
    use hiraeth_store::{principal::Principal, sqs::SqsQueue, test_support::SqsTestStore};
    use serde::Deserialize;

    use super::{
        MessageAttributeValue, calculate_message_attributes_md5, load_queue_from_url,
        parse_queue_url, parse_request_body, queue_url,
    };
    use crate::error::SqsError;

    fn resolved_request(body: &[u8]) -> ResolvedRequest {
        ResolvedRequest {
            request_id: "test-request-id".to_string(),
            request: IncomingRequest {
                host: "localhost:4566".to_string(),
                method: "POST".to_string(),
                path: "/".to_string(),
                query: None,
                headers: HashMap::new(),
                body: body.to_vec(),
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
                        .with_ymd_and_hms(2026, 4, 6, 12, 0, 0)
                        .unwrap()
                        .naive_utc(),
                },
            },
            date: Utc.with_ymd_and_hms(2026, 4, 6, 12, 0, 0).unwrap(),
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
                .with_ymd_and_hms(2026, 4, 6, 12, 0, 0)
                .unwrap()
                .naive_utc(),
            updated_at: Utc
                .with_ymd_and_hms(2026, 4, 6, 12, 0, 0)
                .unwrap()
                .naive_utc(),
            ..Default::default()
        }
    }

    #[derive(Debug, Deserialize, PartialEq, Eq)]
    #[serde(rename_all = "PascalCase")]
    struct TestRequest {
        queue_url: String,
    }

    #[test]
    fn parse_queue_url_extracts_region_from_aws_style_host() {
        let queue_id = parse_queue_url(
            "http://sqs.us-east-1.amazonaws.com/123456789012/orders",
            "us-west-2",
        )
        .expect("queue url should parse");

        assert_eq!(queue_id.account_id, "123456789012");
        assert_eq!(queue_id.name, "orders");
        assert_eq!(queue_id.region, "us-east-1");
    }

    #[test]
    fn parse_queue_url_falls_back_to_request_region_for_local_hosts() {
        let queue_id = parse_queue_url("http://localhost/123456789012/orders", "us-east-1")
            .expect("queue url should parse");

        assert_eq!(queue_id.account_id, "123456789012");
        assert_eq!(queue_id.name, "orders");
        assert_eq!(queue_id.region, "us-east-1");
    }

    #[test]
    fn parse_request_body_deserializes_json_body() {
        let request =
            resolved_request(br#"{"QueueUrl":"http://localhost:4566/123456789012/orders"}"#);

        let request_body =
            parse_request_body::<TestRequest>(&request).expect("request body should parse");

        assert_eq!(
            request_body,
            TestRequest {
                queue_url: "http://localhost:4566/123456789012/orders".to_string()
            }
        );
    }

    #[test]
    fn parse_request_body_rejects_invalid_json() {
        let request =
            resolved_request(br#"{"QueueUrl":"http://localhost:4566/123456789012/orders""#);

        let result = parse_request_body::<TestRequest>(&request);

        assert!(matches!(result, Err(SqsError::BadRequest(_))));
    }

    #[tokio::test]
    async fn load_queue_from_url_returns_matching_queue() {
        let store = SqsTestStore::with_queue(queue());
        let request = resolved_request(br#"{}"#);

        let queue = load_queue_from_url(
            &request,
            &store,
            "http://localhost:4566/123456789012/orders",
        )
        .await
        .expect("queue should load");

        assert_eq!(queue.id, 42);
        assert_eq!(queue.name, "orders");
    }

    #[tokio::test]
    async fn load_queue_from_url_returns_not_found_for_missing_queue() {
        let store = SqsTestStore::default();
        let request = resolved_request(br#"{}"#);

        let result = load_queue_from_url(
            &request,
            &store,
            "http://localhost:4566/123456789012/orders",
        )
        .await;

        assert_eq!(result, Err(SqsError::QueueNotFound));
    }

    #[test]
    fn queue_url_formats_emulator_queue_url() {
        assert_eq!(
            queue_url("localhost:4566", "123456789012", "orders"),
            "http://localhost:4566/123456789012/orders"
        );
    }

    #[test]
    fn calculates_md5_for_string_message_attributes() {
        let mut attributes = BTreeMap::new();
        attributes.insert(
            "trace_id".to_string(),
            MessageAttributeValue {
                data_type: "String".to_string(),
                string_value: Some("abc123".to_string()),
                binary_value: None,
            },
        );

        let digest =
            calculate_message_attributes_md5(&attributes).expect("message attributes should hash");

        assert_eq!(digest, "853c383c82274bde6eac88d91ee96efe");
    }

    #[test]
    fn calculates_md5_for_binary_message_attributes() {
        let mut attributes = BTreeMap::new();
        attributes.insert(
            "payload".to_string(),
            MessageAttributeValue {
                data_type: "Binary".to_string(),
                string_value: None,
                binary_value: Some("AQID".to_string()),
            },
        );

        let digest =
            calculate_message_attributes_md5(&attributes).expect("message attributes should hash");

        assert_eq!(digest, "a7c5b51b9587ba6b55ff17e3b9408909");
    }

    #[test]
    fn calculates_md5_for_hash_map_message_attributes_in_name_order() {
        let attributes = HashMap::from([
            (
                "trace_id".to_string(),
                MessageAttributeValue {
                    data_type: "String".to_string(),
                    string_value: Some("abc123".to_string()),
                    binary_value: None,
                },
            ),
            (
                "tenant".to_string(),
                MessageAttributeValue {
                    data_type: "String".to_string(),
                    string_value: Some("acme".to_string()),
                    binary_value: None,
                },
            ),
        ]);

        let digest =
            calculate_message_attributes_md5(&attributes).expect("message attributes should hash");

        assert_eq!(digest, "dbf9f8110dff50952a8b7b0d4fc539f2");
    }
}
