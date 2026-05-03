use std::collections::HashMap;

use async_trait::async_trait;
use chrono::Utc;
use hiraeth_core::{
    AwsActionPayloadFormat, AwsActionPayloadParseError, AwsActionResponseFormat, ResolvedRequest,
    TypedAwsAction,
    auth::AuthorizationCheck,
    tracing::{TraceContext, TraceRecorder},
};
use hiraeth_store::sns::{SnsStore, SnsSubscription};
use serde::{Deserialize, Serialize};

use super::action_support::{parse_payload_error, query_payload_format};
use crate::error::SnsError;

const SNS_XMLNS: &str = "http://sns.amazonaws.com/doc/2010-03-31/";

pub(crate) struct SubscribeAction;

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub(crate) struct SubscribeRequest {
    topic_arn: String,
    protocol: String,
    endpoint: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "PascalCase")]
pub(crate) struct SubscribeResult {
    subscription_arn: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "PascalCase")]
pub(crate) struct SubscribeResponse {
    #[serde(rename = "@xmlns")]
    xmlns: &'static str,
    subscribe_result: SubscribeResult,
    response_metadata: ResponseMetadata,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "PascalCase")]
pub(crate) struct ResponseMetadata {
    request_id: String,
}

async fn handle_subscribe_typed<S: SnsStore>(
    request: &ResolvedRequest,
    store: &S,
    request_body: SubscribeRequest,
) -> Result<SubscribeResponse, SnsError> {
    let topic = store
        .get_topic(&request_body.topic_arn)
        .await
        .map_err(|e| SnsError::InternalError(e.to_string()))?
        .ok_or(SnsError::TopicNotFound)?;

    if request_body.protocol != "sqs" {
        return Err(SnsError::BadRequest(format!(
            "Protocol '{}' is not supported in this slice",
            request_body.protocol
        )));
    }

    let subscription_arn = format!(
        "arn:aws:sns:{}:{}:{}:{}",
        topic.region,
        topic.account_id,
        topic.name,
        uuid::Uuid::new_v4()
    );

    let subscription = SnsSubscription {
        id: 0,
        topic_arn: request_body.topic_arn,
        protocol: request_body.protocol,
        endpoint: request_body.endpoint,
        owner_account_id: request.auth_context.principal.account_id.clone(),
        subscription_arn: subscription_arn.clone(),
        created_at: Utc::now().naive_utc(),
    };

    store.create_subscription(subscription).await?;

    Ok(SubscribeResponse {
        xmlns: SNS_XMLNS,
        subscribe_result: SubscribeResult { subscription_arn },
        response_metadata: ResponseMetadata {
            request_id: request.request_id.clone(),
        },
    })
}

#[async_trait]
impl<S> TypedAwsAction<S> for SubscribeAction
where
    S: SnsStore + Send + Sync,
{
    type Request = SubscribeRequest;
    type Response = SubscribeResponse;
    type Error = SnsError;

    fn name(&self) -> &'static str {
        "Subscribe"
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
        request_body: &SubscribeRequest,
        _store: &S,
    ) -> Result<(), SnsError> {
        if request_body.topic_arn.is_empty() {
            return Err(SnsError::BadRequest("TopicArn is required".to_string()));
        }
        if request_body.protocol.is_empty() {
            return Err(SnsError::BadRequest("Protocol is required".to_string()));
        }
        if request_body.endpoint.is_empty() {
            return Err(SnsError::BadRequest("Endpoint is required".to_string()));
        }
        Ok(())
    }

    async fn handle(
        &self,
        request: ResolvedRequest,
        request_body: SubscribeRequest,
        store: &S,
        trace_context: &TraceContext,
        trace_recorder: &dyn TraceRecorder,
    ) -> Result<SubscribeResponse, SnsError> {
        let attributes = HashMap::from([
            ("topic_arn".to_string(), request_body.topic_arn.clone()),
            ("protocol".to_string(), request_body.protocol.clone()),
            ("endpoint".to_string(), request_body.endpoint.clone()),
        ]);

        trace_context
            .record_result_span(
                trace_recorder,
                "sns.subscription.create",
                "sns",
                attributes,
                async { handle_subscribe_typed(&request, store, request_body).await },
            )
            .await
    }

    async fn resolve_authorization(
        &self,
        request: &ResolvedRequest,
        _payload: SubscribeRequest,
        store: &S,
    ) -> Result<AuthorizationCheck, SnsError> {
        crate::auth::resolve_authorization("sns:Subscribe", request, store).await
    }
}
