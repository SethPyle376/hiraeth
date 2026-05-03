use std::collections::HashMap;

use async_trait::async_trait;
use chrono::Utc;
use hiraeth_core::{
    AwsActionPayloadFormat, AwsActionPayloadParseError, AwsActionResponseFormat, ResolvedRequest,
    TypedAwsAction,
    auth::{AuthorizationCheck, Policy, PolicyPrincipal, authorize_cross_service},
    tracing::{TraceContext, TraceRecorder},
};
use hiraeth_store::sns::SnsStore;
use hiraeth_store::sqs::SqsStore;
use serde::{Deserialize, Serialize};

use super::action_support::{parse_payload_error, query_payload_format};
use crate::{error::SnsError, store::SnsServiceStore};

const SNS_XMLNS: &str = "http://sns.amazonaws.com/doc/2010-03-31/";

pub(crate) struct PublishAction;

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub(crate) struct PublishRequest {
    topic_arn: String,
    message: String,
    #[serde(default)]
    subject: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename = "PublishResult")]
pub(crate) struct PublishResult {
    message_id: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename = "PublishResponse")]
pub(crate) struct PublishResponse {
    #[serde(rename = "@xmlns")]
    xmlns: &'static str,
    #[serde(rename = "PublishResult")]
    publish_result: PublishResult,
    #[serde(rename = "ResponseMetadata")]
    response_metadata: ResponseMetadata,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "PascalCase")]
pub(crate) struct ResponseMetadata {
    request_id: String,
}

async fn handle_publish_typed<SS, QS>(
    request: &ResolvedRequest,
    store: &SnsServiceStore<SS, QS>,
    request_body: PublishRequest,
    trace_context: &TraceContext,
    trace_recorder: &dyn TraceRecorder,
) -> Result<PublishResponse, SnsError>
where
    SS: SnsStore + Send + Sync,
    QS: SqsStore + Send + Sync,
{
    let topic = store
        .get_topic(&request_body.topic_arn)
        .await?
        .ok_or(SnsError::TopicNotFound)?;

    let subscriptions = store
        .list_subscriptions_by_topic(&request_body.topic_arn)
        .await?;

    let message_id = uuid::Uuid::new_v4().to_string();

    for subscription in subscriptions {
        let result = if subscription.protocol == "sqs" {
            deliver_to_sqs(
                store,
                &subscription,
                &topic,
                &request_body,
                &message_id,
                trace_context,
                trace_recorder,
            )
            .await
        } else {
            let timer = trace_context.start_span();
            let ctx = trace_context.child_context(&timer);
            let attributes = HashMap::from([
                ("topic_arn".to_string(), request_body.topic_arn.clone()),
                ("protocol".to_string(), subscription.protocol.clone()),
                ("endpoint".to_string(), subscription.endpoint.clone()),
            ]);
            ctx.record_span_or_warn(
                trace_recorder,
                timer,
                "sns.message.deliver",
                "sns",
                "ok",
                attributes,
            )
            .await;
            Ok(())
        };

        if let Err(ref e) = result {
            tracing::warn!(
                topic_arn = %request_body.topic_arn,
                endpoint = %subscription.endpoint,
                error = %e,
                "failed to deliver SNS message to subscriber"
            );
        }
    }

    Ok(PublishResponse {
        xmlns: SNS_XMLNS,
        publish_result: PublishResult { message_id },
        response_metadata: ResponseMetadata {
            request_id: request.request_id.clone(),
        },
    })
}

async fn deliver_to_sqs<SS, QS>(
    store: &SnsServiceStore<SS, QS>,
    subscription: &hiraeth_store::sns::SnsSubscription,
    topic: &hiraeth_store::sns::SnsTopic,
    request_body: &PublishRequest,
    message_id: &str,
    trace_context: &TraceContext,
    trace_recorder: &dyn TraceRecorder,
) -> Result<(), SnsError>
where
    SS: SnsStore + Send + Sync,
    QS: SqsStore + Send + Sync,
{
    let queue_id = crate::actions::action_support::parse_sqs_endpoint_arn(&subscription.endpoint)
        .ok_or_else(|| SnsError::BadRequest(format!("Invalid SQS endpoint: {}", subscription.endpoint)))?;

    let sqs_queue = store
        .sqs_store
        .get_queue(&queue_id.name, &queue_id.region, &queue_id.account_id)
        .await?
        .ok_or_else(|| SnsError::BadRequest(format!("SQS queue not found: {}", subscription.endpoint)))?;

    let queue_arn = format!(
        "arn:aws:sqs:{}:{}:{}",
        sqs_queue.region, sqs_queue.account_id, sqs_queue.name
    );

    let resource_policy: Policy = if sqs_queue.policy.is_empty() || sqs_queue.policy == "{}" {
        Policy {
            version: "2012-10-17".to_string(),
            statement: vec![],
        }
    } else {
        serde_json::from_str(&sqs_queue.policy)
            .map_err(|e| SnsError::InternalError(format!("invalid queue policy: {}", e)))?
    };

    let caller = PolicyPrincipal::Service("sns.amazonaws.com".to_string());
    let eval_result =
        authorize_cross_service(&caller, "sqs:SendMessage", &queue_arn, &resource_policy);

    let decision = match eval_result {
        hiraeth_core::auth::PolicyEvalResult::Allowed => hiraeth_router::AuthorizationResult::Allow,
        _ => hiraeth_router::AuthorizationResult::Deny,
    };

    let effective = match store.auth_mode {
        hiraeth_iam::AuthorizationMode::Enforce => decision,
        _ => hiraeth_router::AuthorizationResult::Allow,
    };

    // Authz span is started from the publish context and recorded first.
    let authz_timer = trace_context.start_span();
    let authz_context = trace_context.child_context(&authz_timer);
    let authz_status = decision.as_trace_status();
    authz_context
        .record_span_or_warn(
            trace_recorder,
            authz_timer,
            "authz.evaluate",
            "sns",
            authz_status,
            HashMap::from([
                ("action".to_string(), "sqs:SendMessage".to_string()),
                ("resource".to_string(), queue_arn.clone()),
                (
                    "mode".to_string(),
                    match store.auth_mode {
                        hiraeth_iam::AuthorizationMode::Enforce => "enforce",
                        hiraeth_iam::AuthorizationMode::Audit => "audit",
                        hiraeth_iam::AuthorizationMode::Off => "off",
                    }
                    .to_string(),
                ),
                (
                    "effective_result".to_string(),
                    effective.as_trace_status().to_string(),
                ),
                ("cross_service".to_string(), "true".to_string()),
            ]),
        )
        .await;

    if effective != hiraeth_router::AuthorizationResult::Allow {
        return Err(SnsError::NotAuthorizedToQueue(queue_arn));
    }

    // Delivery span is a child of the authz span.
    let deliver_timer = authz_context.start_span();
    let deliver_context = authz_context.child_context(&deliver_timer);

    let raw_delivery = subscription.raw_message_delivery.as_deref() == Some("true");

    let body = if raw_delivery {
        request_body.message.clone()
    } else {
        serde_json::json!({
            "Type": "Notification",
            "MessageId": message_id,
            "TopicArn": format!("arn:aws:sns:{}:{}:{}", topic.region, topic.account_id, topic.name),
            "Subject": request_body.subject.as_deref().unwrap_or(""),
            "Message": request_body.message,
            "Timestamp": Utc::now().to_rfc3339(),
        })
        .to_string()
    };

    let enqueue_result = hiraeth_sqs::operations::enqueue_message(
        &store.sqs_store,
        &sqs_queue,
        body,
        None,
        None,
        None,
        None,
        Utc::now(),
    )
    .await;

    let deliver_status = if enqueue_result.is_ok() { "ok" } else { "error" };
    let deliver_attributes = HashMap::from([
        ("topic_arn".to_string(), request_body.topic_arn.clone()),
        ("protocol".to_string(), subscription.protocol.clone()),
        ("endpoint".to_string(), subscription.endpoint.clone()),
    ]);

    deliver_context
        .record_span_or_warn(
            trace_recorder,
            deliver_timer,
            "sns.message.deliver",
            "sns",
            deliver_status,
            deliver_attributes,
        )
        .await;

    enqueue_result
        .map(|_message_id| ())
        .map_err(|e| SnsError::InternalError(format!("sqs delivery failed: {}", e)))
}

#[async_trait]
impl<SS, QS> TypedAwsAction<SnsServiceStore<SS, QS>> for PublishAction
where
    SS: SnsStore + Send + Sync,
    QS: SqsStore + Send + Sync,
{
    type Request = PublishRequest;
    type Response = PublishResponse;
    type Error = SnsError;

    fn name(&self) -> &'static str {
        "Publish"
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
        request_body: &PublishRequest,
        _store: &SnsServiceStore<SS, QS>,
    ) -> Result<(), SnsError> {
        if request_body.topic_arn.is_empty() {
            return Err(SnsError::BadRequest("TopicArn is required".to_string()));
        }
        if request_body.message.is_empty() {
            return Err(SnsError::BadRequest("Message is required".to_string()));
        }
        Ok(())
    }

    async fn handle(
        &self,
        request: ResolvedRequest,
        request_body: PublishRequest,
        store: &SnsServiceStore<SS, QS>,
        trace_context: &TraceContext,
        trace_recorder: &dyn TraceRecorder,
    ) -> Result<PublishResponse, SnsError> {
        let attributes = HashMap::from([
            ("topic_arn".to_string(), request_body.topic_arn.clone()),
            (
                "message_bytes".to_string(),
                request_body.message.len().to_string(),
            ),
        ]);

        let timer = trace_context.start_span();
        let publish_context = trace_context.child_context(&timer);

        let result = handle_publish_typed(
            &request,
            store,
            request_body,
            &publish_context,
            trace_recorder,
        )
        .await;

        let status = if result.is_ok() { "ok" } else { "error" };
        trace_context
            .record_span_or_warn(
                trace_recorder,
                timer,
                "sns.message.publish",
                "sns",
                status,
                attributes,
            )
            .await;

        result
    }

    async fn resolve_authorization(
        &self,
        request: &ResolvedRequest,
        _payload: PublishRequest,
        store: &SnsServiceStore<SS, QS>,
    ) -> Result<AuthorizationCheck, SnsError> {
        crate::auth::resolve_authorization("sns:Publish", request, &store.sns_store).await
    }
}
