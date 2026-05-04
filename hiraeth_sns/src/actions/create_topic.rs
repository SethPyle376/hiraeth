use std::collections::HashMap;

use async_trait::async_trait;
use chrono::Utc;
use hiraeth_core::{
    AwsActionPayloadFormat, AwsActionPayloadParseError, AwsActionResponseFormat, ResolvedRequest,
    TypedAwsAction,
    auth::AuthorizationCheck,
    tracing::{TraceContext, TraceRecorder},
};
use hiraeth_store::sns::{SnsStore, SnsTopic};
use serde::{Deserialize, Serialize, de};

use super::action_support::{SnsAttributes, parse_payload_error, query_payload_format};
use crate::error::SnsError;

const SNS_XMLNS: &str = "http://sns.amazonaws.com/doc/2010-03-31/";

pub(crate) struct CreateTopicAction;

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub(crate) struct CreateTopicRequest {
    name: String,
    #[serde(flatten, default)]
    attributes: SnsAttributes,
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

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "PascalCase")]
pub(crate) struct ResponseMetadata {
    request_id: String,
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
        created_at: now,
    };

    store.create_topic(topic).await?;

    Ok(CreateTopicResponse {
        xmlns: SNS_XMLNS,
        create_topic_result: CreateTopicResult { topic_arn },
        response_metadata: ResponseMetadata {
            request_id: request.request_id.clone(),
        },
    })
}

#[async_trait]
impl<S> TypedAwsAction<S> for CreateTopicAction
where
    S: SnsStore + Send + Sync,
{
    type Request = CreateTopicRequest;
    type Response = CreateTopicResponse;
    type Error = SnsError;

    fn name(&self) -> &'static str {
        "CreateTopic"
    }

    fn payload_format(&self) -> AwsActionPayloadFormat {
        query_payload_format()
    }

    fn response_format(&self) -> AwsActionResponseFormat {
        AwsActionResponseFormat::Xml
    }

    fn parse_error(&self, error: AwsActionPayloadParseError) -> SnsError {
        parse_payload_error(error)
    }

    async fn validate(
        &self,
        _request: &ResolvedRequest,
        request_body: &CreateTopicRequest,
        _store: &S,
    ) -> Result<(), SnsError> {
        if request_body.name.is_empty() {
            return Err(SnsError::BadRequest("Name is required".to_string()));
        }
        Ok(())
    }

    async fn handle(
        &self,
        request: ResolvedRequest,
        request_body: CreateTopicRequest,
        store: &S,
        trace_context: &TraceContext,
        trace_recorder: &dyn TraceRecorder,
    ) -> Result<CreateTopicResponse, SnsError> {
        let attributes = HashMap::from([("topic_name".to_string(), request_body.name.clone())]);

        trace_context
            .record_result_span(
                trace_recorder,
                "sns.topic.create",
                "sns",
                attributes,
                async { handle_create_topic_typed(&request, store, request_body).await },
            )
            .await
    }

    async fn resolve_authorization(
        &self,
        request: &ResolvedRequest,
        _payload: CreateTopicRequest,
        _store: &S,
    ) -> Result<AuthorizationCheck, SnsError> {
        Ok(AuthorizationCheck {
            action: "sns:CreateTopic".to_string(),
            resource: format!(
                "arn:aws:sns:{}:{}:*",
                request.region, request.auth_context.principal.account_id
            ),
            resource_policy: None,
        })
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use chrono::{TimeZone, Utc};
    use hiraeth_core::{AuthContext, ResolvedRequest, TypedAwsAction};
    use hiraeth_http::IncomingRequest;
    use hiraeth_store::{principal::Principal, sns::SnsTopic, test_support::SnsTestStore};

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
    async fn validation_rejects_empty_name() {
        let store = SnsTestStore::default();
        let request = resolved_request("Name=");
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
