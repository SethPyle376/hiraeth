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
    let topic = SnsTopic {
        id: 0,
        name: request_body.name,
        region,
        account_id,
        display_name: request_body
            .attributes
            .get("DisplayName")
            .unwrap_or_default()
            .to_string(),
        policy: request_body
            .attributes
            .get("Policy")
            .unwrap_or("{}")
            .to_string(),
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
    use super::CreateTopicRequest;

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
