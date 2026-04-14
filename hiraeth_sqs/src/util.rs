use std::collections::BTreeMap;

use base64::Engine;
use hiraeth_auth::ResolvedRequest;
use hiraeth_store::sqs::{SqsQueue, SqsStore};
use serde::{Deserialize, Serialize, de::DeserializeOwned};

use crate::error::SqsError;

pub(crate) struct QueueId {
    pub name: String,
    pub region: String,
    pub account_id: String,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "PascalCase")]
pub(crate) struct MessageAttributeValue {
    pub data_type: String,
    pub string_value: Option<String>,
    pub binary_value: Option<String>,
}

pub(crate) fn parse_queue_url(queue_url: &str, default_region: &str) -> Option<QueueId> {
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
    serde_json::from_slice(&request.request.body).map_err(|e| SqsError::BadRequest(e.to_string()))
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
        .ok_or_else(|| SqsError::QueueNotFound)
}

pub(crate) fn queue_url(host: &str, account_id: &str, queue_name: &str) -> String {
    format!("http://{host}/{account_id}/{queue_name}")
}

pub(crate) fn calculate_message_attributes_md5<'a>(
    message_attributes: impl IntoIterator<Item = (&'a String, &'a MessageAttributeValue)>,
) -> Result<String, SqsError> {
    let mut buffer = Vec::new();
    let mut message_attributes = message_attributes.into_iter().collect::<Vec<_>>();
    message_attributes.sort_by(|(left_name, _), (right_name, _)| left_name.cmp(right_name));

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

pub(crate) fn serialize_message_attributes<'a>(
    message_attributes: impl IntoIterator<Item = (&'a String, &'a MessageAttributeValue)>,
) -> Result<String, SqsError> {
    let ordered_attributes = message_attributes
        .into_iter()
        .map(|(name, value)| (name.as_str(), value))
        .collect::<BTreeMap<_, _>>();

    serde_json::to_string(&ordered_attributes).map_err(|e| SqsError::BadRequest(e.to_string()))
}

pub(crate) fn extract_aws_trace_header<'a>(
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

#[cfg(test)]
mod tests {
    use std::collections::{BTreeMap, HashMap};

    use chrono::{TimeZone, Utc};
    use hiraeth_auth::{AuthContext, ResolvedRequest};
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
