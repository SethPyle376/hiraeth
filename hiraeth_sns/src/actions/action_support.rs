use std::collections::HashMap;

use hiraeth_core::{AwsActionPayloadFormat, AwsActionPayloadParseError};
use hiraeth_sqs::util::QueueId;

use crate::error::SnsError;

pub(super) fn query_payload_format() -> AwsActionPayloadFormat {
    AwsActionPayloadFormat::AwsQuery
}

pub(super) fn parse_payload_error(error: AwsActionPayloadParseError) -> SnsError {
    match error {
        AwsActionPayloadParseError::AwsQuery(error) => SnsError::BadRequest(error.to_string()),
        AwsActionPayloadParseError::Json(error) => SnsError::from(error),
    }
}

/// Parse an SQS endpoint ARN (`arn:aws:sqs:{region}:{account_id}:{queue_name}`).
pub(super) fn parse_sqs_endpoint_arn(endpoint: &str) -> Option<QueueId> {
    let parts: Vec<&str> = endpoint.split(':').collect();
    if parts.len() == 6 && parts[0] == "arn" && parts[1] == "aws" && parts[2] == "sqs" {
        return Some(QueueId {
            region: parts[3].to_string(),
            account_id: parts[4].to_string(),
            name: parts[5].to_string(),
        });
    }

    None
}

/// Reusable `HashMap<String, String>` that deserializes from SNS `Attributes.entry.N.key/value` pairs.
///
/// Use with `#[serde(flatten, default)]` on a request struct so that any unmatched query keys
/// are collected here and parsed into a clean attribute map.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(super) struct SnsAttributes {
    inner: HashMap<String, String>,
}

impl SnsAttributes {
    pub(super) fn get(&self, key: &str) -> Option<&str> {
        self.inner.get(key).map(String::as_str)
    }

    pub(super) fn is_empty(&self) -> bool {
        self.inner.is_empty()
    }

    pub(super) fn len(&self) -> usize {
        self.inner.len()
    }
}

impl<'de> serde::Deserialize<'de> for SnsAttributes {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let raw = HashMap::<String, String>::deserialize(deserializer)?;
        let mut entries = HashMap::<String, (Option<String>, Option<String>)>::new();

        for (key, value) in raw {
            if let Some(rest) = key.strip_prefix("Attributes.entry.") {
                if let Some(dot_pos) = rest.find('.') {
                    let index = &rest[..dot_pos];
                    let suffix = &rest[dot_pos + 1..];
                    let entry = entries.entry(index.to_string()).or_insert((None, None));
                    match suffix {
                        "key" => entry.0 = Some(value),
                        "value" => entry.1 = Some(value),
                        _ => {}
                    }
                }
            }
        }

        let inner = entries
            .into_values()
            .filter_map(|(k, v)| Some((k?, v?)))
            .collect();

        Ok(SnsAttributes { inner })
    }
}

#[cfg(test)]
mod tests {
    use super::{SnsAttributes, parse_sqs_endpoint_arn};

    #[test]
    fn sns_attributes_deserializes_from_flat_query_keys() {
        let encoded = "Attributes.entry.1.key=DisplayName&Attributes.entry.1.value=MyDisplay&Attributes.entry.2.key=Policy&Attributes.entry.2.value=%7B%7D";
        let attrs: SnsAttributes = serde_urlencoded::from_str(encoded).expect("should deserialize");

        assert_eq!(attrs.len(), 2);
        assert_eq!(attrs.get("DisplayName"), Some("MyDisplay"));
        assert_eq!(attrs.get("Policy"), Some("{}"));
    }

    #[test]
    fn sns_attributes_is_empty_when_no_attribute_keys_present() {
        let attrs: SnsAttributes = serde_urlencoded::from_str("").expect("should deserialize");
        assert!(attrs.is_empty());
    }

    #[test]
    fn sns_attributes_ignores_incomplete_entries() {
        let encoded = "Attributes.entry.1.key=DisplayName";
        let attrs: SnsAttributes = serde_urlencoded::from_str(encoded).expect("should deserialize");
        assert!(attrs.is_empty());
    }

    #[test]
    fn sns_attributes_ignores_non_attribute_keys() {
        let encoded = "Name=test-topic&Action=CreateTopic";
        let attrs: SnsAttributes = serde_urlencoded::from_str(encoded).expect("should deserialize");
        assert!(attrs.is_empty());
    }

    #[test]
    fn parse_sqs_endpoint_arn_extracts_arn_components() {
        let queue_id = parse_sqs_endpoint_arn("arn:aws:sqs:us-east-1:123456789012:my-queue")
            .expect("should parse queue arn");

        assert_eq!(queue_id.name, "my-queue");
        assert_eq!(queue_id.region, "us-east-1");
        assert_eq!(queue_id.account_id, "123456789012");
    }

    #[test]
    fn parse_sqs_endpoint_arn_returns_none_for_invalid_arn() {
        assert!(parse_sqs_endpoint_arn("not-an-arn").is_none());
    }
}
