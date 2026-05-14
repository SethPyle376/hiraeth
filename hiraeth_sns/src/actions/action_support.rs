use std::collections::HashMap;

use hiraeth_core::{AwsActionPayloadFormat, AwsActionPayloadParseError};
use hiraeth_sqs::util::QueueId;
use serde::Serialize;

use crate::error::SnsError;

pub(crate) const SNS_XMLNS: &str = "http://sns.amazonaws.com/doc/2010-03-31/";
pub(super) const MAX_MESSAGE_BYTES: usize = 262_144;
pub(super) const MAX_SUBJECT_CHARS: usize = 100;
const MAX_TOPIC_NAME_CHARS: usize = 256;
const MAX_TAGS_PER_RESOURCE: usize = 50;
const MAX_TAG_KEY_LENGTH: usize = 128;
const MAX_TAG_VALUE_LENGTH: usize = 256;

/// All valid SNS topic attribute names accepted by `CreateTopic` and `SetTopicAttributes`.
///
/// Attributes that are not currently stored/used are included so that calls referencing them
/// pass validation rather than returning an "Unsupported attribute name" error.
pub(super) const VALID_TOPIC_ATTRIBUTES: &[&str] = &[
    // Stored / functional attributes
    "ApplicationSuccessFeedbackRoleArn",
    "ApplicationSuccessFeedbackSampleRate",
    "ApplicationFailureFeedbackRoleArn",
    "ArchivePolicy",
    "ContentBasedDeduplication",
    "DataProtectionPolicy",
    "DeliveryPolicy",
    "DisplayName",
    "FifoTopic",
    "FirehoseSuccessFeedbackRoleArn",
    "FirehoseSuccessFeedbackSampleRate",
    "FirehoseFailureFeedbackRoleArn",
    "HTTPFailureFeedbackRoleArn",
    "HTTPSuccessFeedbackRoleArn",
    "HTTPSuccessFeedbackSampleRate",
    "KmsMasterKeyId",
    "LambdaSuccessFeedbackRoleArn",
    "LambdaSuccessFeedbackSampleRate",
    "LambdaFailureFeedbackRoleArn",
    "Policy",
    "SignatureVersion",
    "SQSFailureFeedbackRoleArn",
    "SQSSuccessFeedbackRoleArn",
    "SQSSuccessFeedbackSampleRate",
    "TracingConfig",
];

pub(super) fn is_valid_topic_attribute(name: &str) -> bool {
    VALID_TOPIC_ATTRIBUTES.contains(&name)
}

pub(super) const VALID_SUBSCRIPTION_ATTRIBUTES: &[&str] = &[
    "DeliveryPolicy",
    "FilterPolicy",
    "FilterPolicyScope",
    "RawMessageDelivery",
    "RedrivePolicy",
    "SubscriptionRoleArn",
    "ReplayPolicy",
];

pub(super) fn is_valid_subscription_attribute(name: &str) -> bool {
    VALID_SUBSCRIPTION_ATTRIBUTES.contains(&name)
}

pub(super) fn validate_topic_name(name: &str, fifo_topic: Option<&str>) -> Result<(), SnsError> {
    let length = name.chars().count();
    if length == 0 || length > MAX_TOPIC_NAME_CHARS {
        return Err(SnsError::BadRequest(format!(
            "Topic name must be between 1 and {MAX_TOPIC_NAME_CHARS} characters"
        )));
    }

    if !name
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.')
    {
        return Err(SnsError::BadRequest(
            "Topic name may only contain letters, numbers, hyphens, underscores, and periods"
                .to_string(),
        ));
    }

    let fifo_requested = fifo_topic
        .map(|value| value.eq_ignore_ascii_case("true"))
        .unwrap_or(false);
    if fifo_requested && !name.ends_with(".fifo") {
        return Err(SnsError::BadRequest(
            "FIFO topic names must end with .fifo".to_string(),
        ));
    }
    if !fifo_requested && name.ends_with(".fifo") {
        return Err(SnsError::BadRequest(
            "FIFO topic names require FifoTopic=true".to_string(),
        ));
    }

    Ok(())
}

pub(super) fn validate_json_attribute(
    attribute_name: &str,
    attribute_value: &str,
) -> Result<(), SnsError> {
    match attribute_name {
        "Policy"
        | "DeliveryPolicy"
        | "DataProtectionPolicy"
        | "ArchivePolicy"
        | "FilterPolicy"
        | "RedrivePolicy"
        | "ReplayPolicy" => {
            serde_json::from_str::<serde_json::Value>(attribute_value).map_err(|error| {
                SnsError::BadRequest(format!("{} must be valid JSON: {}", attribute_name, error))
            })?;
        }
        _ => {}
    }

    Ok(())
}

pub(super) fn validate_raw_message_delivery(value: &str) -> Result<(), SnsError> {
    match value {
        "true" | "false" | "True" | "False" | "TRUE" | "FALSE" => Ok(()),
        _ => Err(SnsError::BadRequest(
            "RawMessageDelivery must be true or false".to_string(),
        )),
    }
}

pub(super) fn validate_publish_payload(
    message: &str,
    subject: Option<&str>,
) -> Result<(), SnsError> {
    if message.is_empty() {
        return Err(SnsError::BadRequest("Message is required".to_string()));
    }
    if message.len() > MAX_MESSAGE_BYTES {
        return Err(SnsError::BadRequest(format!(
            "Message must be at most {MAX_MESSAGE_BYTES} bytes"
        )));
    }
    if let Some(subject) = subject {
        if subject.chars().count() > MAX_SUBJECT_CHARS {
            return Err(SnsError::BadRequest(format!(
                "Subject must be at most {MAX_SUBJECT_CHARS} characters"
            )));
        }
        if subject.chars().any(char::is_control) {
            return Err(SnsError::BadRequest(
                "Subject cannot contain control characters".to_string(),
            ));
        }
    }

    Ok(())
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "PascalCase")]
pub(crate) struct ResponseMetadata {
    pub request_id: String,
}

pub(super) fn query_payload_format() -> AwsActionPayloadFormat {
    AwsActionPayloadFormat::AwsQuery
}

pub(crate) fn parse_payload_error(error: AwsActionPayloadParseError) -> SnsError {
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

pub(crate) struct TopicId {
    pub region: String,
    pub account_id: String,
    pub name: String,
}

pub(super) fn parse_sns_topic_arn(arn: &str) -> Option<TopicId> {
    let parts: Vec<&str> = arn.split(':').collect();
    if parts.len() == 6
        && parts[0] == "arn"
        && parts[1] == "aws"
        && parts[2] == "sns"
        && !parts[3].is_empty()
        && !parts[4].is_empty()
        && !parts[5].is_empty()
    {
        return Some(TopicId {
            region: parts[3].to_string(),
            account_id: parts[4].to_string(),
            name: parts[5].to_string(),
        });
    }

    None
}

pub(super) fn validate_topic_arn(arn: &str, field_name: &str) -> Result<(), SnsError> {
    if arn.is_empty() {
        return Err(SnsError::BadRequest(format!("{field_name} is required")));
    }

    parse_sns_topic_arn(arn)
        .map(|_| ())
        .ok_or_else(|| SnsError::BadRequest(format!("Invalid {field_name} format")))
}

pub(super) fn validate_subscription_arn(arn: &str) -> Result<(), SnsError> {
    if arn.is_empty() {
        return Err(SnsError::BadRequest(
            "SubscriptionArn is required".to_string(),
        ));
    }

    let parts: Vec<&str> = arn.split(':').collect();
    if parts.len() == 7
        && parts[0] == "arn"
        && parts[1] == "aws"
        && parts[2] == "sns"
        && !parts[3].is_empty()
        && !parts[4].is_empty()
        && !parts[5].is_empty()
        && !parts[6].is_empty()
    {
        return Ok(());
    }

    Err(SnsError::BadRequest(
        "Invalid SubscriptionArn format".to_string(),
    ))
}

pub(super) fn topic_policy_attribute_value(
    stored_policy: &str,
    topic_arn: &str,
    account_id: &str,
) -> String {
    if stored_policy.trim().is_empty() || stored_policy.trim() == "{}" {
        return default_topic_policy(topic_arn, account_id);
    }

    stored_policy.to_string()
}

fn default_topic_policy(topic_arn: &str, account_id: &str) -> String {
    serde_json::json!({
        "Version": "2008-10-17",
        "Id": "__default_policy_ID",
        "Statement": [
            {
                "Sid": "__default_statement_ID",
                "Effect": "Allow",
                "Principal": {
                    "AWS": "*"
                },
                "Action": [
                    "SNS:GetTopicAttributes",
                    "SNS:SetTopicAttributes",
                    "SNS:AddPermission",
                    "SNS:RemovePermission",
                    "SNS:DeleteTopic",
                    "SNS:Subscribe",
                    "SNS:ListSubscriptionsByTopic",
                    "SNS:Publish"
                ],
                "Resource": topic_arn,
                "Condition": {
                    "StringEquals": {
                        "AWS:SourceOwner": account_id
                    }
                }
            }
        ]
    })
    .to_string()
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

    pub(super) fn keys(&self) -> impl Iterator<Item = &str> {
        self.inner.keys().map(String::as_str)
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
            if let Some(rest) = key.strip_prefix("Attributes.entry.")
                && let Some(dot_pos) = rest.find('.')
            {
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

        let inner = entries
            .into_values()
            .filter_map(|(k, v)| Some((k?, v?)))
            .collect();

        Ok(SnsAttributes { inner })
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(super) struct SnsTags {
    inner: HashMap<String, String>,
}

impl SnsTags {
    pub(super) fn as_map(&self) -> &HashMap<String, String> {
        &self.inner
    }

    pub(super) fn into_inner(self) -> HashMap<String, String> {
        self.inner
    }

    pub(super) fn len(&self) -> usize {
        self.inner.len()
    }

    pub(super) fn keys(&self) -> impl Iterator<Item = &String> {
        self.inner.keys()
    }
}

impl<'de> serde::Deserialize<'de> for SnsTags {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let raw = HashMap::<String, String>::deserialize(deserializer)?;
        let mut entries = HashMap::<String, (Option<String>, Option<String>)>::new();

        for (key, value) in raw {
            if let Some(rest) = key.strip_prefix("Tags.member.")
                && let Some(dot_pos) = rest.find('.')
            {
                let index = &rest[..dot_pos];
                let suffix = &rest[dot_pos + 1..];
                let entry = entries.entry(index.to_string()).or_insert((None, None));
                match suffix {
                    "Key" => entry.0 = Some(value),
                    "Value" => entry.1 = Some(value),
                    _ => {}
                }
            }
        }

        let inner = entries
            .into_values()
            .filter_map(|(k, v)| Some((k?, v?)))
            .collect();

        Ok(SnsTags { inner })
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(super) struct SnsTagKeys {
    inner: Vec<String>,
}

impl SnsTagKeys {
    pub(super) fn as_slice(&self) -> &[String] {
        &self.inner
    }

    pub(super) fn into_inner(self) -> Vec<String> {
        self.inner
    }

    pub(super) fn len(&self) -> usize {
        self.inner.len()
    }

    pub(super) fn join(&self, separator: &str) -> String {
        self.inner.join(separator)
    }
}

impl<'de> serde::Deserialize<'de> for SnsTagKeys {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let raw = HashMap::<String, String>::deserialize(deserializer)?;
        let mut entries = raw
            .into_iter()
            .filter_map(|(key, value)| {
                key.strip_prefix("TagKeys.member.")
                    .map(|index| (index.to_string(), value))
            })
            .collect::<Vec<_>>();
        entries.sort_by(|(left, _), (right, _)| left.cmp(right));

        Ok(SnsTagKeys {
            inner: entries.into_iter().map(|(_, value)| value).collect(),
        })
    }
}

pub(super) fn validate_tags(
    tags: &HashMap<String, String>,
    allow_empty: bool,
) -> Result<(), SnsError> {
    if !allow_empty && tags.is_empty() {
        return Err(SnsError::BadRequest(
            "Tags must contain at least one entry".to_string(),
        ));
    }

    if tags.len() > MAX_TAGS_PER_RESOURCE {
        return Err(SnsError::BadRequest(format!(
            "A resource can have at most {MAX_TAGS_PER_RESOURCE} tags"
        )));
    }

    for (key, value) in tags {
        validate_tag_key(key)?;
        if value.chars().count() > MAX_TAG_VALUE_LENGTH {
            return Err(SnsError::BadRequest(format!(
                "Tag value for '{}' must be at most {} characters",
                key, MAX_TAG_VALUE_LENGTH
            )));
        }
    }

    Ok(())
}

pub(super) fn validate_tag_keys(tag_keys: &[String], allow_empty: bool) -> Result<(), SnsError> {
    if !allow_empty && tag_keys.is_empty() {
        return Err(SnsError::BadRequest(
            "TagKeys must contain at least one entry".to_string(),
        ));
    }

    if tag_keys.len() > MAX_TAGS_PER_RESOURCE {
        return Err(SnsError::BadRequest(format!(
            "TagKeys can contain at most {MAX_TAGS_PER_RESOURCE} entries"
        )));
    }

    for key in tag_keys {
        validate_tag_key(key)?;
    }

    Ok(())
}

fn validate_tag_key(key: &str) -> Result<(), SnsError> {
    let key_length = key.chars().count();

    if key_length == 0 || key_length > MAX_TAG_KEY_LENGTH {
        return Err(SnsError::BadRequest(format!(
            "Tag keys must be between 1 and {} characters",
            MAX_TAG_KEY_LENGTH
        )));
    }

    if key.starts_with("aws:") {
        return Err(SnsError::BadRequest(
            "Tag keys cannot start with the reserved aws: prefix".to_string(),
        ));
    }

    if key.chars().any(char::is_control) {
        return Err(SnsError::BadRequest(
            "Tag keys cannot contain control characters".to_string(),
        ));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use crate::error::SnsError;

    use super::{
        SnsAttributes, SnsTagKeys, SnsTags, parse_sns_topic_arn, parse_sqs_endpoint_arn,
        topic_policy_attribute_value, validate_subscription_arn, validate_tag_keys, validate_tags,
        validate_topic_arn, validate_topic_name,
    };

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
    fn sns_tags_deserializes_member_style_tags() {
        let encoded = "Tags.member.1.Key=team&Tags.member.1.Value=platform&Tags.member.2.Key=env&Tags.member.2.Value=dev";
        let tags: SnsTags = serde_urlencoded::from_str(encoded).expect("should deserialize");

        assert_eq!(tags.len(), 2);
        assert_eq!(tags.into_inner()["team"], "platform");
    }

    #[test]
    fn sns_tag_keys_deserializes_member_style_keys() {
        let encoded = "TagKeys.member.1=team&TagKeys.member.2=env";
        let tag_keys: SnsTagKeys = serde_urlencoded::from_str(encoded).expect("should deserialize");

        assert_eq!(tag_keys.into_inner(), vec!["team", "env"]);
    }

    #[test]
    fn validates_reserved_tag_prefix() {
        let result = validate_tags(
            &[("aws:reserved".to_string(), "value".to_string())]
                .into_iter()
                .collect(),
            false,
        );

        assert!(matches!(result, Err(SnsError::BadRequest(_))));
    }

    #[test]
    fn validates_empty_tag_key_list() {
        let result = validate_tag_keys(&[], false);

        assert!(matches!(result, Err(SnsError::BadRequest(_))));
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

    #[test]
    fn parse_sns_topic_arn_rejects_missing_topic_name() {
        assert!(parse_sns_topic_arn("arn:aws:sns:us-east-1:123456789012:").is_none());
    }

    #[test]
    fn validate_topic_arn_rejects_invalid_shape() {
        let result = validate_topic_arn("not-an-arn", "TopicArn");

        assert!(matches!(result, Err(SnsError::BadRequest(_))));
    }

    #[test]
    fn validate_subscription_arn_rejects_topic_arn_shape() {
        let result = validate_subscription_arn("arn:aws:sns:us-east-1:123456789012:test-topic");

        assert!(matches!(result, Err(SnsError::BadRequest(_))));
    }

    #[test]
    fn validate_topic_name_rejects_spaces() {
        let result = validate_topic_name("bad topic", None);

        assert!(matches!(result, Err(SnsError::BadRequest(_))));
    }

    #[test]
    fn validate_topic_name_accepts_fifo_topic_with_suffix() {
        assert!(validate_topic_name("events.fifo", Some("true")).is_ok());
    }

    #[test]
    fn validate_topic_name_rejects_fifo_suffix_without_attribute() {
        let result = validate_topic_name("events.fifo", None);

        assert!(matches!(result, Err(SnsError::BadRequest(_))));
    }

    #[test]
    fn topic_policy_attribute_value_renders_default_policy_for_empty_object() {
        let policy = topic_policy_attribute_value(
            "{}",
            "arn:aws:sns:us-east-1:123456789012:test-topic",
            "123456789012",
        );
        let parsed: serde_json::Value = serde_json::from_str(&policy).unwrap();

        assert_eq!(parsed["Version"], "2008-10-17");
        assert_eq!(
            parsed["Statement"][0]["Resource"],
            "arn:aws:sns:us-east-1:123456789012:test-topic"
        );
        assert_eq!(
            parsed["Statement"][0]["Condition"]["StringEquals"]["AWS:SourceOwner"],
            "123456789012"
        );
    }

    #[test]
    fn topic_policy_attribute_value_preserves_explicit_policy() {
        let policy = r#"{"Version":"2012-10-17","Statement":[]}"#;

        assert_eq!(
            topic_policy_attribute_value(
                policy,
                "arn:aws:sns:us-east-1:123456789012:test-topic",
                "123456789012",
            ),
            policy
        );
    }
}
