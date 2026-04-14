use std::collections::HashMap;

use hiraeth_auth::ResolvedRequest;
use hiraeth_router::ServiceResponse;
use hiraeth_store::sqs::{SqsQueue, SqsQueueAttributeUpdate, SqsStore};
use serde::{Deserialize, Serialize};

use crate::{error::SqsError, util};

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct GetQueueAttributesRequest {
    pub queue_url: String,
    pub attribute_names: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "PascalCase")]
pub struct GetQueueAttributesResponse {
    pub attributes: HashMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct QueueAttributeValues {
    pub visibility_timeout_seconds: i64,
    pub delay_seconds: i64,
    pub maximum_message_size: i64,
    pub message_retention_period_seconds: i64,
    pub receive_message_wait_time_seconds: i64,
    pub policy: String,
    pub redrive_policy: String,
    pub fifo_queue: bool,
    pub content_based_deduplication: bool,
    pub kms_master_key_id: Option<String>,
    pub kms_data_key_reuse_period_seconds: i64,
    pub deduplication_scope: String,
    pub fifo_throughput_limit: String,
    pub redrive_allow_policy: String,
    pub sqs_managed_sse_enabled: bool,
}

const CREATE_QUEUE_ATTRIBUTES: &[&str] = &[
    "VisibilityTimeout",
    "DelaySeconds",
    "MaximumMessageSize",
    "MessageRetentionPeriod",
    "ReceiveMessageWaitTimeSeconds",
    "Policy",
    "RedrivePolicy",
    "FifoQueue",
    "ContentBasedDeduplication",
    "KmsMasterKeyId",
    "KmsDataKeyReusePeriodSeconds",
    "DeduplicationScope",
    "FifoThroughputLimit",
    "RedriveAllowPolicy",
    "SqsManagedSseEnabled",
];

const SET_QUEUE_ATTRIBUTES: &[&str] = &[
    "VisibilityTimeout",
    "DelaySeconds",
    "MaximumMessageSize",
    "MessageRetentionPeriod",
    "ReceiveMessageWaitTimeSeconds",
    "Policy",
    "RedrivePolicy",
    "ContentBasedDeduplication",
    "KmsMasterKeyId",
    "KmsDataKeyReusePeriodSeconds",
    "DeduplicationScope",
    "FifoThroughputLimit",
    "RedriveAllowPolicy",
    "SqsManagedSseEnabled",
];

impl Default for QueueAttributeValues {
    fn default() -> Self {
        Self {
            visibility_timeout_seconds: 30,
            delay_seconds: 0,
            maximum_message_size: 1048576,
            message_retention_period_seconds: 345600,
            receive_message_wait_time_seconds: 0,
            policy: "{}".to_string(),
            redrive_policy: "{}".to_string(),
            fifo_queue: false,
            content_based_deduplication: false,
            kms_master_key_id: None,
            kms_data_key_reuse_period_seconds: 300,
            deduplication_scope: "queue".to_string(),
            fifo_throughput_limit: "perQueue".to_string(),
            redrive_allow_policy: "{}".to_string(),
            sqs_managed_sse_enabled: false,
        }
    }
}

impl QueueAttributeValues {
    pub(crate) fn from_attribute_map(
        attributes: &HashMap<String, String>,
    ) -> Result<Self, SqsError> {
        validate_supported_attributes(attributes, CREATE_QUEUE_ATTRIBUTES)?;
        let defaults = Self::default();

        Ok(Self {
            visibility_timeout_seconds: get_i64_attribute(
                attributes,
                "VisibilityTimeout",
                defaults.visibility_timeout_seconds,
                0,
                43200,
            )?,
            delay_seconds: get_i64_attribute(
                attributes,
                "DelaySeconds",
                defaults.delay_seconds,
                0,
                900,
            )?,
            maximum_message_size: get_i64_attribute(
                attributes,
                "MaximumMessageSize",
                defaults.maximum_message_size,
                1024,
                1048576,
            )?,
            message_retention_period_seconds: get_i64_attribute(
                attributes,
                "MessageRetentionPeriod",
                defaults.message_retention_period_seconds,
                60,
                1209600,
            )?,
            receive_message_wait_time_seconds: get_i64_attribute(
                attributes,
                "ReceiveMessageWaitTimeSeconds",
                defaults.receive_message_wait_time_seconds,
                0,
                20,
            )?,
            policy: get_json_string_attribute(attributes, "Policy", &defaults.policy)?,
            redrive_policy: get_json_string_attribute(
                attributes,
                "RedrivePolicy",
                &defaults.redrive_policy,
            )?,
            fifo_queue: get_bool_attribute(attributes, "FifoQueue", defaults.fifo_queue)?,
            content_based_deduplication: get_bool_attribute(
                attributes,
                "ContentBasedDeduplication",
                defaults.content_based_deduplication,
            )?,
            kms_master_key_id: attributes.get("KmsMasterKeyId").cloned(),
            kms_data_key_reuse_period_seconds: get_i64_attribute(
                attributes,
                "KmsDataKeyReusePeriodSeconds",
                defaults.kms_data_key_reuse_period_seconds,
                60,
                86400,
            )?,
            deduplication_scope: get_allowed_string_attribute(
                attributes,
                "DeduplicationScope",
                &defaults.deduplication_scope,
                &["queue", "messageGroup"],
            )?,
            fifo_throughput_limit: get_allowed_string_attribute(
                attributes,
                "FifoThroughputLimit",
                &defaults.fifo_throughput_limit,
                &["perQueue", "perMessageGroupId"],
            )?,
            redrive_allow_policy: get_json_string_attribute(
                attributes,
                "RedriveAllowPolicy",
                &defaults.redrive_allow_policy,
            )?,
            sqs_managed_sse_enabled: get_bool_attribute(
                attributes,
                "SqsManagedSseEnabled",
                defaults.sqs_managed_sse_enabled,
            )?,
        })
    }
}

pub(crate) fn parse_queue_attribute_update(
    attributes: &HashMap<String, String>,
) -> Result<SqsQueueAttributeUpdate, SqsError> {
    validate_supported_attributes(attributes, SET_QUEUE_ATTRIBUTES)?;

    Ok(SqsQueueAttributeUpdate {
        visibility_timeout_seconds: parse_i64_attribute(attributes, "VisibilityTimeout", 0, 43200)?,
        delay_seconds: parse_i64_attribute(attributes, "DelaySeconds", 0, 900)?,
        maximum_message_size: parse_i64_attribute(attributes, "MaximumMessageSize", 1024, 1048576)?,
        message_retention_period_seconds: parse_i64_attribute(
            attributes,
            "MessageRetentionPeriod",
            60,
            1209600,
        )?,
        receive_message_wait_time_seconds: parse_i64_attribute(
            attributes,
            "ReceiveMessageWaitTimeSeconds",
            0,
            20,
        )?,
        policy: parse_json_string_attribute(attributes, "Policy")?,
        redrive_policy: parse_json_string_attribute(attributes, "RedrivePolicy")?,
        content_based_deduplication: parse_bool_attribute(attributes, "ContentBasedDeduplication")?,
        kms_master_key_id: attributes.contains_key("KmsMasterKeyId").then(|| {
            attributes
                .get("KmsMasterKeyId")
                .filter(|value| !value.is_empty())
                .cloned()
        }),
        kms_data_key_reuse_period_seconds: parse_i64_attribute(
            attributes,
            "KmsDataKeyReusePeriodSeconds",
            60,
            86400,
        )?,
        deduplication_scope: parse_allowed_string_attribute(
            attributes,
            "DeduplicationScope",
            &["queue", "messageGroup"],
        )?,
        fifo_throughput_limit: parse_allowed_string_attribute(
            attributes,
            "FifoThroughputLimit",
            &["perQueue", "perMessageGroupId"],
        )?,
        redrive_allow_policy: parse_json_string_attribute(attributes, "RedriveAllowPolicy")?,
        sqs_managed_sse_enabled: parse_bool_attribute(attributes, "SqsManagedSseEnabled")?,
    })
}

pub(crate) async fn get_queue_attributes<S: SqsStore>(
    request: &ResolvedRequest,
    store: &S,
) -> Result<ServiceResponse, SqsError> {
    let attributes_request = util::parse_request_body::<GetQueueAttributesRequest>(request)?;

    let queue = util::load_queue_from_url(request, store, &attributes_request.queue_url).await?;
    let attributes =
        collect_queue_attributes(store, &queue, &attributes_request.attribute_names).await?;

    Ok(ServiceResponse {
        status_code: 200,
        headers: vec![],
        body: serde_json::to_vec(&GetQueueAttributesResponse { attributes })
            .map_err(|e| SqsError::BadRequest(e.to_string()))?,
    })
}

pub(crate) async fn collect_queue_attributes<S: SqsStore>(
    store: &S,
    queue: &SqsQueue,
    requested_attributes: &[String],
) -> Result<HashMap<String, String>, SqsError> {
    let mut attributes = HashMap::<String, String>::new();

    insert_string_attribute(
        &mut attributes,
        requested_attributes,
        "Policy",
        &queue.policy,
    );
    insert_i64_attribute(
        &mut attributes,
        requested_attributes,
        "VisibilityTimeout",
        queue.visibility_timeout_seconds,
    );
    insert_i64_attribute(
        &mut attributes,
        requested_attributes,
        "MaximumMessageSize",
        queue.maximum_message_size,
    );
    insert_i64_attribute(
        &mut attributes,
        requested_attributes,
        "MessageRetentionPeriod",
        queue.message_retention_period_seconds,
    );

    if is_requested_attribute("ApproximateNumberOfMessages", requested_attributes) {
        let message_count = store
            .get_message_count(queue.id)
            .await
            .map_err(|e| SqsError::InternalError(e.to_string()))?;
        attributes.insert(
            "ApproximateNumberOfMessages".to_string(),
            message_count.to_string(),
        );
    }

    if is_requested_attribute(
        "ApproximateNumberOfMessagesNotVisible",
        requested_attributes,
    ) {
        let visible_message_count = store
            .get_visible_message_count(queue.id)
            .await
            .map_err(|e| SqsError::InternalError(e.to_string()))?;
        let message_count = store
            .get_message_count(queue.id)
            .await
            .map_err(|e| SqsError::InternalError(e.to_string()))?;
        attributes.insert(
            "ApproximateNumberOfMessagesNotVisible".to_string(),
            (message_count - visible_message_count).to_string(),
        );
    }

    if is_requested_attribute("CreatedTimestamp", requested_attributes) {
        attributes.insert(
            "CreatedTimestamp".to_string(),
            queue.created_at.and_utc().timestamp_millis().to_string(),
        );
    }

    if is_requested_attribute("LastModifiedTimestamp", requested_attributes) {
        attributes.insert(
            "LastModifiedTimestamp".to_string(),
            queue.updated_at.and_utc().timestamp_millis().to_string(),
        );
    }

    if is_requested_attribute("QueueArn", requested_attributes) {
        attributes.insert(
            "QueueArn".to_string(),
            format!(
                "arn:aws:sqs:{}:{}:{}",
                queue.region, queue.account_id, queue.name
            ),
        );
    }

    if is_requested_attribute("ApproximateNumberOfMessagesDelayed", requested_attributes) {
        let delayed_message_count = store
            .get_messages_delayed_count(queue.id)
            .await
            .map_err(|e| SqsError::InternalError(e.to_string()))?;
        attributes.insert(
            "ApproximateNumberOfMessagesDelayed".to_string(),
            delayed_message_count.to_string(),
        );
    }

    insert_i64_attribute(
        &mut attributes,
        requested_attributes,
        "DelaySeconds",
        queue.delay_seconds,
    );
    insert_i64_attribute(
        &mut attributes,
        requested_attributes,
        "ReceiveMessageWaitTimeSeconds",
        queue.receive_message_wait_time_seconds,
    );
    insert_string_attribute(
        &mut attributes,
        requested_attributes,
        "RedrivePolicy",
        &queue.redrive_policy,
    );
    insert_bool_attribute(
        &mut attributes,
        requested_attributes,
        "FifoQueue",
        queue.queue_type == "fifo",
    );
    insert_bool_attribute(
        &mut attributes,
        requested_attributes,
        "ContentBasedDeduplication",
        queue.content_based_deduplication,
    );
    if let Some(kms_master_key_id) = &queue.kms_master_key_id {
        insert_string_attribute(
            &mut attributes,
            requested_attributes,
            "KmsMasterKeyId",
            kms_master_key_id,
        );
    }
    insert_i64_attribute(
        &mut attributes,
        requested_attributes,
        "KmsDataKeyReusePeriodSeconds",
        queue.kms_data_key_reuse_period_seconds,
    );
    insert_string_attribute(
        &mut attributes,
        requested_attributes,
        "DeduplicationScope",
        &queue.deduplication_scope,
    );
    insert_string_attribute(
        &mut attributes,
        requested_attributes,
        "FifoThroughputLimit",
        &queue.fifo_throughput_limit,
    );
    insert_string_attribute(
        &mut attributes,
        requested_attributes,
        "RedriveAllowPolicy",
        &queue.redrive_allow_policy,
    );
    insert_bool_attribute(
        &mut attributes,
        requested_attributes,
        "SqsManagedSseEnabled",
        queue.sqs_managed_sse_enabled,
    );

    Ok(attributes)
}

fn validate_supported_attributes(
    attributes: &HashMap<String, String>,
    supported_attributes: &[&str],
) -> Result<(), SqsError> {
    for attribute in attributes.keys() {
        if !supported_attributes.contains(&attribute.as_str()) {
            return Err(SqsError::BadRequest(format!(
                "Unsupported queue attribute: {}",
                attribute
            )));
        }
    }

    Ok(())
}

fn get_i64_attribute(
    attributes: &HashMap<String, String>,
    name: &str,
    default: i64,
    min: i64,
    max: i64,
) -> Result<i64, SqsError> {
    Ok(parse_i64_attribute(attributes, name, min, max)?.unwrap_or(default))
}

fn parse_i64_attribute(
    attributes: &HashMap<String, String>,
    name: &str,
    min: i64,
    max: i64,
) -> Result<Option<i64>, SqsError> {
    let Some(raw) = attributes.get(name) else {
        return Ok(None);
    };

    let value = raw.parse::<i64>().map_err(|_| {
        SqsError::BadRequest(format!(
            "{} must be an integer between {} and {}",
            name, min, max
        ))
    })?;

    if !(min..=max).contains(&value) {
        return Err(SqsError::BadRequest(format!(
            "{} must be between {} and {}",
            name, min, max
        )));
    }

    Ok(Some(value))
}

fn get_bool_attribute(
    attributes: &HashMap<String, String>,
    name: &str,
    default: bool,
) -> Result<bool, SqsError> {
    Ok(parse_bool_attribute(attributes, name)?.unwrap_or(default))
}

fn parse_bool_attribute(
    attributes: &HashMap<String, String>,
    name: &str,
) -> Result<Option<bool>, SqsError> {
    let Some(raw) = attributes.get(name) else {
        return Ok(None);
    };

    match raw.to_ascii_lowercase().as_str() {
        "true" => Ok(Some(true)),
        "false" => Ok(Some(false)),
        _ => Err(SqsError::BadRequest(format!(
            "{} must be either true or false",
            name
        ))),
    }
}

fn get_json_string_attribute(
    attributes: &HashMap<String, String>,
    name: &str,
    default: &str,
) -> Result<String, SqsError> {
    Ok(parse_json_string_attribute(attributes, name)?.unwrap_or_else(|| default.to_string()))
}

fn parse_json_string_attribute(
    attributes: &HashMap<String, String>,
    name: &str,
) -> Result<Option<String>, SqsError> {
    let Some(raw) = attributes.get(name) else {
        return Ok(None);
    };

    serde_json::from_str::<serde_json::Value>(raw)
        .map_err(|e| SqsError::BadRequest(format!("{} must be valid JSON: {}", name, e)))?;

    Ok(Some(raw.clone()))
}

fn parse_allowed_string_attribute(
    attributes: &HashMap<String, String>,
    name: &str,
    allowed: &[&str],
) -> Result<Option<String>, SqsError> {
    let Some(raw) = attributes.get(name) else {
        return Ok(None);
    };

    if allowed.contains(&raw.as_str()) {
        Ok(Some(raw.clone()))
    } else {
        Err(SqsError::BadRequest(format!(
            "{} must be one of: {}",
            name,
            allowed.join(", ")
        )))
    }
}

fn get_allowed_string_attribute(
    attributes: &HashMap<String, String>,
    name: &str,
    default: &str,
    allowed: &[&str],
) -> Result<String, SqsError> {
    Ok(parse_allowed_string_attribute(attributes, name, allowed)?
        .unwrap_or_else(|| default.to_string()))
}

fn insert_i64_attribute(
    attributes: &mut HashMap<String, String>,
    requested_attributes: &[String],
    name: &str,
    value: i64,
) {
    if is_requested_attribute(name, requested_attributes) {
        attributes.insert(name.to_string(), value.to_string());
    }
}

fn insert_bool_attribute(
    attributes: &mut HashMap<String, String>,
    requested_attributes: &[String],
    name: &str,
    value: bool,
) {
    if is_requested_attribute(name, requested_attributes) {
        attributes.insert(name.to_string(), value.to_string());
    }
}

fn insert_string_attribute(
    attributes: &mut HashMap<String, String>,
    requested_attributes: &[String],
    name: &str,
    value: &str,
) {
    if is_requested_attribute(name, requested_attributes) {
        attributes.insert(name.to_string(), value.to_string());
    }
}

fn is_requested_attribute(attribute_name: &str, requested_attributes: &[String]) -> bool {
    requested_attributes
        .iter()
        .any(|requested| requested == attribute_name || requested == "All")
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use chrono::{TimeZone, Utc};
    use hiraeth_auth::{AuthContext, ResolvedRequest};
    use hiraeth_http::IncomingRequest;
    use hiraeth_router::ServiceResponse;
    use hiraeth_store::{principal::Principal, sqs::SqsQueue, test_support::SqsTestStore};
    use serde_json::Value;

    use super::{QueueAttributeValues, collect_queue_attributes, get_queue_attributes};
    use crate::error::SqsError;

    fn resolved_request(body: &str) -> ResolvedRequest {
        let mut headers = HashMap::new();
        headers.insert(
            "x-amz-target".to_string(),
            "AmazonSQS.GetQueueAttributes".to_string(),
        );

        ResolvedRequest {
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
                    created_at: Utc
                        .with_ymd_and_hms(2026, 4, 4, 12, 0, 0)
                        .unwrap()
                        .naive_utc(),
                },
            },
            date: Utc.with_ymd_and_hms(2026, 4, 4, 12, 0, 0).unwrap(),
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
            maximum_message_size: 2048,
            message_retention_period_seconds: 345600,
            receive_message_wait_time_seconds: 10,
            policy: r#"{"Statement":[]}"#.to_string(),
            redrive_policy: r#"{"maxReceiveCount":"5"}"#.to_string(),
            content_based_deduplication: true,
            kms_master_key_id: Some("alias/test".to_string()),
            kms_data_key_reuse_period_seconds: 600,
            deduplication_scope: "messageGroup".to_string(),
            fifo_throughput_limit: "perMessageGroupId".to_string(),
            redrive_allow_policy: r#"{"redrivePermission":"allowAll"}"#.to_string(),
            sqs_managed_sse_enabled: true,
            created_at: Utc
                .with_ymd_and_hms(2026, 4, 4, 11, 0, 0)
                .unwrap()
                .naive_utc(),
            updated_at: Utc
                .with_ymd_and_hms(2026, 4, 4, 11, 30, 0)
                .unwrap()
                .naive_utc(),
        }
    }

    fn parse_json_body(response: &ServiceResponse) -> Value {
        serde_json::from_slice(&response.body).expect("response body should be valid json")
    }

    fn attribute_names(names: &[&str]) -> Vec<String> {
        names.iter().map(|name| name.to_string()).collect()
    }

    #[test]
    fn queue_attribute_values_uses_defaults_for_missing_attributes() {
        let attributes = QueueAttributeValues::from_attribute_map(&HashMap::new())
            .expect("missing attributes should use defaults");

        assert_eq!(attributes, QueueAttributeValues::default());
    }

    #[test]
    fn queue_attribute_values_parses_supported_attributes() {
        let attributes = QueueAttributeValues::from_attribute_map(&HashMap::from([
            ("VisibilityTimeout".to_string(), "45".to_string()),
            ("DelaySeconds".to_string(), "5".to_string()),
            ("MaximumMessageSize".to_string(), "2048".to_string()),
            ("MessageRetentionPeriod".to_string(), "86400".to_string()),
            (
                "ReceiveMessageWaitTimeSeconds".to_string(),
                "10".to_string(),
            ),
            (
                "Policy".to_string(),
                r#"{"Version":"2012-10-17"}"#.to_string(),
            ),
            (
                "RedrivePolicy".to_string(),
                r#"{"maxReceiveCount":"5"}"#.to_string(),
            ),
            ("FifoQueue".to_string(), "true".to_string()),
            ("ContentBasedDeduplication".to_string(), "true".to_string()),
            ("KmsMasterKeyId".to_string(), "alias/test".to_string()),
            (
                "KmsDataKeyReusePeriodSeconds".to_string(),
                "600".to_string(),
            ),
            ("DeduplicationScope".to_string(), "messageGroup".to_string()),
            (
                "FifoThroughputLimit".to_string(),
                "perMessageGroupId".to_string(),
            ),
            (
                "RedriveAllowPolicy".to_string(),
                r#"{"redrivePermission":"allowAll"}"#.to_string(),
            ),
            ("SqsManagedSseEnabled".to_string(), "true".to_string()),
        ]))
        .expect("supported attributes should parse");

        assert_eq!(
            attributes,
            QueueAttributeValues {
                visibility_timeout_seconds: 45,
                delay_seconds: 5,
                maximum_message_size: 2048,
                message_retention_period_seconds: 86400,
                receive_message_wait_time_seconds: 10,
                policy: r#"{"Version":"2012-10-17"}"#.to_string(),
                redrive_policy: r#"{"maxReceiveCount":"5"}"#.to_string(),
                fifo_queue: true,
                content_based_deduplication: true,
                kms_master_key_id: Some("alias/test".to_string()),
                kms_data_key_reuse_period_seconds: 600,
                deduplication_scope: "messageGroup".to_string(),
                fifo_throughput_limit: "perMessageGroupId".to_string(),
                redrive_allow_policy: r#"{"redrivePermission":"allowAll"}"#.to_string(),
                sqs_managed_sse_enabled: true,
            }
        );
    }

    #[test]
    fn queue_attribute_values_rejects_invalid_values() {
        let result = QueueAttributeValues::from_attribute_map(&HashMap::from([
            ("VisibilityTimeout".to_string(), "not-a-number".to_string()),
            ("DelaySeconds".to_string(), "5".to_string()),
        ]));

        assert!(matches!(result, Err(SqsError::BadRequest(_))));
    }

    #[test]
    fn queue_attribute_values_rejects_unknown_attributes() {
        let result = QueueAttributeValues::from_attribute_map(&HashMap::from([(
            "UnsupportedAttribute".to_string(),
            "value".to_string(),
        )]));

        assert!(matches!(result, Err(SqsError::BadRequest(_))));
    }

    #[tokio::test]
    async fn collect_queue_attributes_returns_all_supported_attributes() {
        let store = SqsTestStore::with_queue(queue()).with_message_counts(7, 3, 2);

        let attributes = collect_queue_attributes(&store, &queue(), &attribute_names(&["All"]))
            .await
            .expect("attributes should collect");

        assert_eq!(attributes["Policy"], r#"{"Statement":[]}"#);
        assert_eq!(attributes["VisibilityTimeout"], "30");
        assert_eq!(attributes["MaximumMessageSize"], "2048");
        assert_eq!(attributes["MessageRetentionPeriod"], "345600");
        assert_eq!(attributes["ApproximateNumberOfMessages"], "7");
        assert_eq!(attributes["ApproximateNumberOfMessagesNotVisible"], "4");
        assert_eq!(attributes["ApproximateNumberOfMessagesDelayed"], "2");
        assert_eq!(
            attributes["CreatedTimestamp"],
            Utc.with_ymd_and_hms(2026, 4, 4, 11, 0, 0)
                .unwrap()
                .timestamp_millis()
                .to_string()
        );
        assert_eq!(
            attributes["LastModifiedTimestamp"],
            Utc.with_ymd_and_hms(2026, 4, 4, 11, 30, 0)
                .unwrap()
                .timestamp_millis()
                .to_string()
        );
        assert_eq!(
            attributes["QueueArn"],
            "arn:aws:sqs:us-east-1:123456789012:orders"
        );
        assert_eq!(attributes["DelaySeconds"], "5");
        assert_eq!(attributes["ReceiveMessageWaitTimeSeconds"], "10");
        assert_eq!(attributes["RedrivePolicy"], r#"{"maxReceiveCount":"5"}"#);
        assert_eq!(attributes["FifoQueue"], "false");
        assert_eq!(attributes["ContentBasedDeduplication"], "true");
        assert_eq!(attributes["KmsMasterKeyId"], "alias/test");
        assert_eq!(attributes["KmsDataKeyReusePeriodSeconds"], "600");
        assert_eq!(attributes["DeduplicationScope"], "messageGroup");
        assert_eq!(attributes["FifoThroughputLimit"], "perMessageGroupId");
        assert_eq!(
            attributes["RedriveAllowPolicy"],
            r#"{"redrivePermission":"allowAll"}"#
        );
        assert_eq!(attributes["SqsManagedSseEnabled"], "true");
    }

    #[tokio::test]
    async fn get_queue_attributes_returns_requested_attributes() {
        let store = SqsTestStore::with_queue(queue()).with_message_counts(7, 3, 2);
        let request = resolved_request(
            r#"{
                "QueueUrl":"http://localhost:4566/123456789012/orders",
                "AttributeNames":[
                    "VisibilityTimeout",
                    "ApproximateNumberOfMessages",
                    "ApproximateNumberOfMessagesNotVisible",
                    "ApproximateNumberOfMessagesDelayed",
                    "QueueArn",
                    "CreatedTimestamp",
                    "ReceiveMessageWaitTimeSeconds"
                ]
            }"#,
        );

        let response = get_queue_attributes(&request, &store)
            .await
            .expect("get queue attributes should succeed");

        assert_eq!(response.status_code, 200);

        let body = parse_json_body(&response);
        let attributes = &body["Attributes"];
        assert_eq!(attributes["VisibilityTimeout"], "30");
        assert_eq!(attributes["ApproximateNumberOfMessages"], "7");
        assert_eq!(attributes["ApproximateNumberOfMessagesNotVisible"], "4");
        assert_eq!(attributes["ApproximateNumberOfMessagesDelayed"], "2");
        assert_eq!(
            attributes["QueueArn"],
            "arn:aws:sqs:us-east-1:123456789012:orders"
        );
        assert_eq!(
            attributes["CreatedTimestamp"],
            Utc.with_ymd_and_hms(2026, 4, 4, 11, 0, 0)
                .unwrap()
                .timestamp_millis()
                .to_string()
        );
        assert_eq!(attributes["ReceiveMessageWaitTimeSeconds"], "10");
    }

    #[tokio::test]
    async fn get_queue_attributes_returns_not_found_for_missing_queue() {
        let store = SqsTestStore::default();
        let request = resolved_request(
            r#"{
                "QueueUrl":"http://localhost:4566/123456789012/orders",
                "AttributeNames":["All"]
            }"#,
        );

        let result = get_queue_attributes(&request, &store).await;

        assert!(matches!(result, Err(SqsError::QueueNotFound)));
    }
}
