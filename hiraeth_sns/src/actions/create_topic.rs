use std::collections::HashMap;

use chrono::Utc;
use hiraeth_core::{ResolvedRequest, auth::AuthorizationCheck};
use hiraeth_store::sns::{SnsStore, SnsTopic};
use serde::{Deserialize, Serialize};

use super::action_support::{
    SnsAttributes, SnsTags, is_valid_topic_attribute, validate_json_attribute, validate_tags,
    validate_topic_name,
};
use crate::{
    actions::action_support::{ResponseMetadata, SNS_XMLNS},
    error::SnsError,
};

pub(crate) struct CreateTopicAction;

hiraeth_core::impl_aws_action! {
    CreateTopicAction<S: SnsStore> {
        request: CreateTopicRequest,
        response: CreateTopicResponse,
        defaults: crate::SnsActionDefaults,
        name: "CreateTopic",
        validate: |_request, payload, _store| {
            validate_topic_name(&payload.name, payload.attributes.get("FifoTopic"))?;
            for key in payload.attributes.keys() {
                if !is_valid_topic_attribute(key) {
                    return Err(SnsError::BadRequest(format!(
                        "Unsupported attribute name: {}",
                        key
                    )));
                }
                if let Some(value) = payload.attributes.get(key) {
                    validate_json_attribute(key, value)?;
                }
            }
            validate_tags(payload.tags.as_map(), true)?;
            Ok(())
        },
        handler: handle_create_topic_typed,
        span: "sns.topic.create",
        span_attrs: |_request, payload, _store| {
            HashMap::from([("topic_name".to_string(), payload.name.clone())])
        },
        authorize: |request, _payload, _store| {
            Ok(AuthorizationCheck {
                action: "sns:CreateTopic".to_string(),
                resource: format!(
                    "arn:aws:sns:{}:{}:*",
                    request.region, request.auth_context.principal.account_id
                ),
                resource_policy: None,
            })
        },
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub(crate) struct CreateTopicRequest {
    name: String,
    #[serde(flatten, default)]
    attributes: SnsAttributes,
    #[serde(flatten, default)]
    tags: SnsTags,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "PascalCase")]
pub(crate) struct CreateTopicResult {
    topic_arn: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "PascalCase")]
pub(crate) struct CreateTopicResponse {
    #[serde(rename = "@xmlns")]
    xmlns: &'static str,
    create_topic_result: CreateTopicResult,
    response_metadata: ResponseMetadata,
}

async fn handle_create_topic_typed<S: SnsStore>(
    request: &ResolvedRequest,
    store: &S,
    request_body: CreateTopicRequest,
) -> Result<CreateTopicResponse, SnsError> {
    let account_id = request.auth_context.principal.account_id.clone();
    let region = request.region.clone();
    let topic_arn = format!(
        "arn:aws:sns:{}:{}:{}",
        region, account_id, request_body.name
    );

    let now = Utc::now().naive_utc();
    let attrs = &request_body.attributes;
    let tags = request_body.tags.into_inner();
    let topic = SnsTopic {
        id: 0,
        name: request_body.name,
        region,
        account_id,
        display_name: attrs.get("DisplayName").map(|s| s.to_string()),
        policy: attrs.get("Policy").unwrap_or("{}").to_string(),
        delivery_policy: attrs.get("DeliveryPolicy").map(|s| s.to_string()),
        fifo_topic: attrs.get("FifoTopic").map(|s| s.to_string()),
        signature_version: attrs.get("SignatureVersion").map(|s| s.to_string()),
        tracing_config: attrs.get("TracingConfig").map(|s| s.to_string()),
        kms_master_key_id: attrs.get("KmsMasterKeyId").map(|s| s.to_string()),
        data_protection_policy: attrs.get("DataProtectionPolicy").map(|s| s.to_string()),
        archive_policy: attrs.get("ArchivePolicy").map(|s| s.to_string()),
        beginning_archive_time: None,
        content_based_deduplication: attrs
            .get("ContentBasedDeduplication")
            .map(|s| s.to_string()),
        created_at: now,
    };

    store.create_topic(topic).await?;
    if !tags.is_empty() {
        store.tag_topic(&topic_arn, tags).await?;
    }

    Ok(CreateTopicResponse {
        xmlns: SNS_XMLNS,
        create_topic_result: CreateTopicResult { topic_arn },
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
    use hiraeth_store::{
        principal::Principal,
        sns::{SnsStore, SnsTopic},
        test_support::SnsTestStore,
    };

    use super::{CreateTopicAction, CreateTopicRequest, handle_create_topic_typed};

    fn resolved_request(body: &str) -> ResolvedRequest {
        let mut headers = HashMap::new();
        headers.insert(
            "x-amz-target".to_string(),
            "AmazonSNS.CreateTopic".to_string(),
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
            display_name: None,
            policy: "{}".to_string(),
            delivery_policy: None,
            fifo_topic: None,
            signature_version: None,
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
            <CreateTopicAction as TypedAwsAction<SnsTestStore>>::name(&CreateTopicAction),
            "CreateTopic"
        );
    }

    #[tokio::test]
    async fn create_topic_with_default_attributes() {
        let store = SnsTestStore::default();
        let request = resolved_request("Name=test-topic");
        let body: CreateTopicRequest = crate::actions::test_support::parse_request_body(&request);

        let response = handle_create_topic_typed(&request, &store, body)
            .await
            .expect("create topic should succeed");

        assert_eq!(
            response.create_topic_result.topic_arn,
            "arn:aws:sns:us-east-1:123456789012:test-topic"
        );

        let created = store.created_topics();
        assert_eq!(created.len(), 1);
        assert_eq!(created[0].name, "test-topic");
        assert_eq!(created[0].region, "us-east-1");
        assert_eq!(created[0].account_id, "123456789012");
        assert_eq!(created[0].policy, "{}");
        assert_eq!(created[0].display_name, None);
        assert_eq!(created[0].delivery_policy, None);
    }

    #[tokio::test]
    async fn create_topic_with_custom_attributes() {
        let store = SnsTestStore::default();
        let request = resolved_request(
            "Name=test-topic&Attributes.entry.1.key=DisplayName&Attributes.entry.1.value=MyDisplay&Attributes.entry.2.key=Policy&Attributes.entry.2.value=%7B%22Statement%22%3A%5B%5D%7D&Attributes.entry.3.key=DeliveryPolicy&Attributes.entry.3.value=%7B%7D",
        );
        let body: CreateTopicRequest = crate::actions::test_support::parse_request_body(&request);

        let response = handle_create_topic_typed(&request, &store, body)
            .await
            .expect("create topic should succeed");

        assert_eq!(
            response.create_topic_result.topic_arn,
            "arn:aws:sns:us-east-1:123456789012:test-topic"
        );

        let created = store.created_topics();
        assert_eq!(created.len(), 1);
        assert_eq!(created[0].name, "test-topic");
        assert_eq!(created[0].display_name, Some("MyDisplay".to_string()));
        assert_eq!(created[0].policy, r#"{"Statement":[]}"#);
        assert_eq!(created[0].delivery_policy, Some("{}".to_string()));
    }

    #[tokio::test]
    async fn create_topic_with_tags() {
        let store = SnsTestStore::default();
        let request = resolved_request(
            "Name=test-topic&Tags.member.1.Key=environment&Tags.member.1.Value=test&Tags.member.2.Key=owner&Tags.member.2.Value=hiraeth",
        );
        let body: CreateTopicRequest = crate::actions::test_support::parse_request_body(&request);

        handle_create_topic_typed(&request, &store, body)
            .await
            .expect("create topic should succeed");

        let tags = store
            .list_topic_tags("arn:aws:sns:us-east-1:123456789012:test-topic")
            .await
            .expect("topic tags should list");
        assert_eq!(tags.get("environment"), Some(&"test".to_string()));
        assert_eq!(tags.get("owner"), Some(&"hiraeth".to_string()));
    }

    #[tokio::test]
    async fn validation_rejects_empty_name() {
        let store = SnsTestStore::default();
        let request = resolved_request("Name=");
        let body: CreateTopicRequest = crate::actions::test_support::parse_request_body(&request);

        let result = CreateTopicAction.validate(&request, &body, &store).await;
        assert!(matches!(result, Err(crate::error::SnsError::BadRequest(_))));
    }

    #[tokio::test]
    async fn validation_rejects_invalid_topic_name() {
        let store = SnsTestStore::default();
        let request = resolved_request("Name=bad topic");
        let body: CreateTopicRequest = crate::actions::test_support::parse_request_body(&request);

        let result = CreateTopicAction.validate(&request, &body, &store).await;
        assert!(matches!(result, Err(crate::error::SnsError::BadRequest(_))));
    }

    #[tokio::test]
    async fn validation_rejects_fifo_name_without_fifo_attribute() {
        let store = SnsTestStore::default();
        let request = resolved_request("Name=test-topic.fifo");
        let body: CreateTopicRequest = crate::actions::test_support::parse_request_body(&request);

        let result = CreateTopicAction.validate(&request, &body, &store).await;
        assert!(matches!(result, Err(crate::error::SnsError::BadRequest(_))));
    }

    #[tokio::test]
    async fn validation_rejects_invalid_policy_json() {
        let store = SnsTestStore::default();
        let request = resolved_request(
            "Name=test-topic&Attributes.entry.1.key=Policy&Attributes.entry.1.value=%7B",
        );
        let body: CreateTopicRequest = crate::actions::test_support::parse_request_body(&request);

        let result = CreateTopicAction.validate(&request, &body, &store).await;
        assert!(matches!(result, Err(crate::error::SnsError::BadRequest(_))));
    }

    #[tokio::test]
    async fn validation_accepts_feedback_attributes() {
        let store = SnsTestStore::default();
        let feedback_attrs = [
            "HTTPSuccessFeedbackRoleArn",
            "HTTPSuccessFeedbackSampleRate",
            "HTTPFailureFeedbackRoleArn",
            "FirehoseSuccessFeedbackRoleArn",
            "FirehoseSuccessFeedbackSampleRate",
            "FirehoseFailureFeedbackRoleArn",
            "LambdaSuccessFeedbackRoleArn",
            "LambdaSuccessFeedbackSampleRate",
            "LambdaFailureFeedbackRoleArn",
            "ApplicationSuccessFeedbackRoleArn",
            "ApplicationSuccessFeedbackSampleRate",
            "ApplicationFailureFeedbackRoleArn",
            "SQSSuccessFeedbackRoleArn",
            "SQSSuccessFeedbackSampleRate",
            "SQSFailureFeedbackRoleArn",
        ];

        for attr in feedback_attrs {
            let request = resolved_request(&format!(
                "Name=test-topic&Attributes.entry.1.key={}&Attributes.entry.1.value=arn:aws:iam::123456789012:role/test",
                attr
            ));
            let body: CreateTopicRequest =
                crate::actions::test_support::parse_request_body(&request);

            let result = CreateTopicAction.validate(&request, &body, &store).await;
            assert!(result.is_ok(), "expected {} to pass validation", attr);
        }
    }

    #[tokio::test]
    async fn validation_rejects_unsupported_attribute() {
        let store = SnsTestStore::default();
        let request = resolved_request(
            "Name=test-topic&Attributes.entry.1.key=UnknownAttr&Attributes.entry.1.value=value",
        );
        let body: CreateTopicRequest = crate::actions::test_support::parse_request_body(&request);

        let result = CreateTopicAction.validate(&request, &body, &store).await;
        assert!(matches!(result, Err(crate::error::SnsError::BadRequest(_))));
    }

    #[tokio::test]
    async fn validation_rejects_reserved_tag_key_prefix() {
        let store = SnsTestStore::default();
        let request = resolved_request(
            "Name=test-topic&Tags.member.1.Key=aws:reserved&Tags.member.1.Value=value",
        );
        let body: CreateTopicRequest = crate::actions::test_support::parse_request_body(&request);

        let result = CreateTopicAction.validate(&request, &body, &store).await;
        assert!(matches!(result, Err(crate::error::SnsError::BadRequest(_))));
    }

    #[test]
    fn deserialize_create_topic_request_with_attributes() {
        let encoded = "Name=test-topic&Attributes.entry.1.key=DisplayName&Attributes.entry.1.value=MyDisplay&Attributes.entry.2.key=Policy&Attributes.entry.2.value=%7B%7D";
        let request: CreateTopicRequest =
            serde_urlencoded::from_str(encoded).expect("should deserialize");

        assert_eq!(request.name, "test-topic");
        assert_eq!(request.attributes.len(), 2);
        assert_eq!(request.attributes.get("DisplayName"), Some("MyDisplay"));
        assert_eq!(request.attributes.get("Policy"), Some("{}"));
        assert!(request.tags.as_map().is_empty());
    }

    #[test]
    fn deserialize_create_topic_request_with_tags() {
        let encoded = "Name=test-topic&Tags.member.1.Key=environment&Tags.member.1.Value=test";
        let request: CreateTopicRequest =
            serde_urlencoded::from_str(encoded).expect("should deserialize");

        assert_eq!(request.name, "test-topic");
        assert_eq!(
            request.tags.as_map().get("environment"),
            Some(&"test".to_string())
        );
    }

    #[test]
    fn deserialize_create_topic_request_without_attributes() {
        let encoded = "Name=test-topic";
        let request: CreateTopicRequest =
            serde_urlencoded::from_str(encoded).expect("should deserialize");

        assert_eq!(request.name, "test-topic");
        assert!(request.attributes.is_empty());
    }

    #[test]
    fn deserialize_create_topic_request_rejects_missing_name() {
        let encoded = "Attributes.entry.1.key=DisplayName&Attributes.entry.1.value=MyDisplay";
        let result: Result<CreateTopicRequest, _> = serde_urlencoded::from_str(encoded);
        assert!(result.is_err());
    }
}
