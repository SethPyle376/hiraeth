use std::collections::BTreeMap;

use base64::Engine;
use serde::{Deserialize, Serialize};

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

pub(crate) fn calculate_message_attributes_md5(
    message_attributes: &BTreeMap<String, MessageAttributeValue>,
) -> Result<String, SqsError> {
    let mut buffer = Vec::new();

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

pub(crate) fn extract_aws_trace_header(
    message_system_attributes: Option<&BTreeMap<String, MessageAttributeValue>>,
) -> Result<Option<String>, SqsError> {
    let Some(message_system_attributes) = message_system_attributes else {
        return Ok(None);
    };

    let Some(trace_header) = message_system_attributes.get("AWSTraceHeader") else {
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
    use std::collections::BTreeMap;

    use super::{MessageAttributeValue, calculate_message_attributes_md5, parse_queue_url};

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
}
