use std::collections::HashMap;

use hiraeth_core::ResolvedRequest;
use hiraeth_store::sns::SnsStore;
use serde::{Deserialize, Serialize};

use super::action_support::{
    ResponseMetadata, SNS_XMLNS, parse_sns_topic_arn, topic_policy_attribute_value,
    validate_topic_arn,
};
use crate::error::SnsError;

pub(crate) struct GetTopicAttributesAction;

hiraeth_core::impl_aws_action! {
    GetTopicAttributesAction<S: SnsStore> {
        request: GetTopicAttributesRequest,
        response: GetTopicAttributesResponse,
        defaults: crate::SnsActionDefaults,
        name: "GetTopicAttributes",
        validate: |_request, payload, _store| {
            validate_topic_arn(&payload.topic_arn, "TopicArn")
        },
        handler: handle_get_topic_attributes_typed,
        span: "sns.topic.get_attributes",
        span_attrs: |_request, payload, _store| {
            HashMap::from([("topic_arn".to_string(), payload.topic_arn.clone())])
        },
        authorize_action: "sns:GetTopicAttributes",
        authorize_with: crate::auth::resolve_authorization,
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub(crate) struct GetTopicAttributesRequest {
    pub topic_arn: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "PascalCase")]
pub(crate) struct GetTopicAttributesResponse {
    #[serde(rename = "@xmlns")]
    xmlns: &'static str,
    get_topic_attributes_result: GetTopicAttributesResult,
    response_metadata: ResponseMetadata,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "PascalCase")]
pub(crate) struct GetTopicAttributesResult {
    attributes: TopicAttributes,
}

#[derive(Debug, Clone, Serialize)]
struct TopicAttributes {
    entry: Vec<AttributeEntry>,
}

#[derive(Debug, Clone, Serialize)]
struct AttributeEntry {
    key: String,
    value: String,
}

async fn handle_get_topic_attributes_typed<S: SnsStore>(
    request: &ResolvedRequest,
    store: &S,
    request_body: GetTopicAttributesRequest,
) -> Result<GetTopicAttributesResponse, SnsError> {
    let topic = store
        .get_topic(&request_body.topic_arn)
        .await?
        .ok_or(SnsError::TopicNotFound)?;

    let topic_arn = format!(
        "arn:aws:sns:{}:{}:{}",
        topic.region, topic.account_id, topic.name
    );

    let mut entries = vec![
        AttributeEntry {
            key: "TopicArn".to_string(),
            value: topic_arn.clone(),
        },
        AttributeEntry {
            key: "Owner".to_string(),
            value: topic.account_id.clone(),
        },
        AttributeEntry {
            key: "Policy".to_string(),
            value: topic_policy_attribute_value(&topic.policy, &topic_arn, &topic.account_id),
        },
    ];

    if let Some(v) = &topic.display_name {
        entries.push(AttributeEntry {
            key: "DisplayName".to_string(),
            value: v.clone(),
        });
    }
    if let Some(v) = &topic.delivery_policy {
        entries.push(AttributeEntry {
            key: "DeliveryPolicy".to_string(),
            value: v.clone(),
        });
    }
    if let Some(v) = &topic.fifo_topic {
        entries.push(AttributeEntry {
            key: "FifoTopic".to_string(),
            value: v.clone(),
        });
    }
    if let Some(v) = &topic.content_based_deduplication {
        entries.push(AttributeEntry {
            key: "ContentBasedDeduplication".to_string(),
            value: v.clone(),
        });
    }
    if let Some(v) = &topic.signature_version {
        entries.push(AttributeEntry {
            key: "SignatureVersion".to_string(),
            value: v.clone(),
        });
    }
    if let Some(v) = &topic.tracing_config {
        entries.push(AttributeEntry {
            key: "TracingConfig".to_string(),
            value: v.clone(),
        });
    }
    if let Some(v) = &topic.kms_master_key_id {
        entries.push(AttributeEntry {
            key: "KmsMasterKeyId".to_string(),
            value: v.clone(),
        });
    }
    if let Some(v) = &topic.data_protection_policy {
        entries.push(AttributeEntry {
            key: "DataProtectionPolicy".to_string(),
            value: v.clone(),
        });
    }
    if let Some(v) = &topic.archive_policy {
        entries.push(AttributeEntry {
            key: "ArchivePolicy".to_string(),
            value: v.clone(),
        });
    }
    if let Some(v) = &topic.beginning_archive_time {
        entries.push(AttributeEntry {
            key: "BeginningArchiveTime".to_string(),
            value: v.clone(),
        });
    }

    Ok(GetTopicAttributesResponse {
        xmlns: SNS_XMLNS,
        get_topic_attributes_result: GetTopicAttributesResult {
            attributes: TopicAttributes { entry: entries },
        },
        response_metadata: ResponseMetadata {
            request_id: request.request_id.clone(),
        },
    })
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use chrono::{TimeZone, Utc};
    use hiraeth_core::{AuthContext, ResolvedRequest, TypedAwsAction};
    use hiraeth_http::IncomingRequest;
    use hiraeth_store::{principal::Principal, sns::SnsTopic, test_support::SnsTestStore};

    use super::{
        GetTopicAttributesAction, GetTopicAttributesRequest, handle_get_topic_attributes_typed,
    };

    fn resolved_request(body: &str) -> ResolvedRequest {
        let mut headers = HashMap::new();
        headers.insert(
            "x-amz-target".to_string(),
            "AmazonSNS.GetTopicAttributes".to_string(),
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
            service: "sns".to_string(),
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
                        .with_ymd_and_hms(2026, 4, 4, 12, 0, 0)
                        .unwrap()
                        .naive_utc(),
                },
            },
            date: Utc.with_ymd_and_hms(2026, 4, 4, 12, 0, 0).unwrap(),
        }
    }

    fn topic() -> SnsTopic {
        SnsTopic {
            id: 1,
            name: "test-topic".to_string(),
            region: "us-east-1".to_string(),
            account_id: "123456789012".to_string(),
            display_name: Some("MyDisplay".to_string()),
            policy: "{}".to_string(),
            delivery_policy: Some("{\"http\":{}}".to_string()),
            fifo_topic: None,
            signature_version: Some("2".to_string()),
            tracing_config: None,
            kms_master_key_id: None,
            data_protection_policy: None,
            archive_policy: None,
            beginning_archive_time: None,
            content_based_deduplication: None,
            created_at: Utc::now().naive_utc(),
        }
    }

    #[test]
    fn reports_expected_action_name() {
        assert_eq!(
            <GetTopicAttributesAction as TypedAwsAction<SnsTestStore>>::name(
                &GetTopicAttributesAction
            ),
            "GetTopicAttributes"
        );
    }

    #[tokio::test]
    async fn get_topic_attributes_returns_expected_attributes() {
        let store = SnsTestStore::with_topic(topic());
        let request = resolved_request("TopicArn=arn:aws:sns:us-east-1:123456789012:test-topic");
        let body: GetTopicAttributesRequest =
            crate::actions::test_support::parse_request_body(&request);

        let response = handle_get_topic_attributes_typed(&request, &store, body)
            .await
            .expect("get topic attributes should succeed");

        let attrs: HashMap<String, String> = response
            .get_topic_attributes_result
            .attributes
            .entry
            .into_iter()
            .map(|e| (e.key, e.value))
            .collect();

        assert_eq!(
            attrs.get("TopicArn"),
            Some(&"arn:aws:sns:us-east-1:123456789012:test-topic".to_string())
        );
        assert_eq!(attrs.get("Owner"), Some(&"123456789012".to_string()));
        let policy = attrs.get("Policy").expect("policy should be present");
        let policy: serde_json::Value =
            serde_json::from_str(policy).expect("policy should be valid json");
        assert_eq!(policy["Version"], "2008-10-17");
        assert_eq!(
            policy["Statement"][0]["Resource"],
            "arn:aws:sns:us-east-1:123456789012:test-topic"
        );
        assert_eq!(
            policy["Statement"][0]["Condition"]["StringEquals"]["AWS:SourceOwner"],
            "123456789012"
        );
        assert_eq!(attrs.get("DisplayName"), Some(&"MyDisplay".to_string()));
        assert_eq!(
            attrs.get("DeliveryPolicy"),
            Some(&"{\"http\":{}}".to_string())
        );
        assert_eq!(attrs.get("SignatureVersion"), Some(&"2".to_string()));
        assert!(!attrs.contains_key("FifoTopic"));
        assert!(!attrs.contains_key("TracingConfig"));
    }

    #[tokio::test]
    async fn topic_not_found_error() {
        let store = SnsTestStore::default();
        let request = resolved_request("TopicArn=arn:aws:sns:us-east-1:123456789012:test-topic");
        let body: GetTopicAttributesRequest =
            crate::actions::test_support::parse_request_body(&request);

        let result = handle_get_topic_attributes_typed(&request, &store, body).await;
        assert!(matches!(result, Err(crate::error::SnsError::TopicNotFound)));
    }

    #[tokio::test]
    async fn validation_rejects_empty_topic_arn() {
        let store = SnsTestStore::default();
        let request = resolved_request("TopicArn=");
        let body: GetTopicAttributesRequest =
            crate::actions::test_support::parse_request_body(&request);

        let result = GetTopicAttributesAction
            .validate(&request, &body, &store)
            .await;
        assert!(matches!(result, Err(crate::error::SnsError::BadRequest(_))));
    }
}
