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
#[serde(rename = "PublishResult", rename_all = "PascalCase")]
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

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use chrono::{TimeZone, Utc};
    use hiraeth_core::{
        AuthContext, ResolvedRequest, TypedAwsAction,
        tracing::{NoopTraceRecorder, TraceContext},
    };
    use hiraeth_http::IncomingRequest;
    use hiraeth_iam::AuthorizationMode;
    use hiraeth_store::{
        principal::Principal,
        sns::{SnsStore, SnsSubscription, SnsTopic},
        test_support::{SnsTestStore, SqsTestStore},
    };

    use super::{PublishAction, PublishRequest, handle_publish_typed};
    use crate::store::SnsServiceStore;

    fn resolved_request(body: &str) -> ResolvedRequest {
        let mut headers = HashMap::new();
        headers.insert(
            "x-amz-target".to_string(),
            "AmazonSNS.Publish".to_string(),
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

    fn subscription() -> SnsSubscription {
        SnsSubscription {
            id: 1,
            topic_arn: "arn:aws:sns:us-east-1:123456789012:test-topic".to_string(),
            protocol: "sqs".to_string(),
            endpoint: "arn:aws:sqs:us-east-1:123456789012:test-queue".to_string(),
            owner_account_id: "123456789012".to_string(),
            subscription_arn: "arn:aws:sns:us-east-1:123456789012:test-topic:uuid-1".to_string(),
            delivery_policy: None,
            filter_policy: None,
            filter_policy_scope: None,
            raw_message_delivery: None,
            redrive_policy: None,
            subscription_role_arn: None,
            replay_policy: None,
            created_at: Utc::now().naive_utc(),
        }
    }

    fn service_store(
        sns: SnsTestStore,
        sqs: SqsTestStore,
    ) -> SnsServiceStore<SnsTestStore, SqsTestStore> {
        SnsServiceStore::new(sns, sqs, AuthorizationMode::Off)
    }

    #[test]
    fn reports_expected_action_name() {
        assert_eq!(
            <PublishAction as TypedAwsAction<SnsServiceStore<SnsTestStore, SqsTestStore>>>::name(
                &PublishAction
            ),
            "Publish"
        );
    }

    #[tokio::test]
    async fn publish_to_topic_with_no_subscriptions() {
        let sns = SnsTestStore::with_topic(topic());
        let sqs = SqsTestStore::default();
        let store = service_store(sns, sqs);
        let request = resolved_request(
            "TopicArn=arn:aws:sns:us-east-1:123456789012:test-topic&Message=hello",
        );
        let body: PublishRequest = crate::actions::test_support::parse_request_body(&request);
        let trace_context = TraceContext::new("test-request-id");

        let response =
            handle_publish_typed(&request, &store, body, &trace_context, &NoopTraceRecorder)
                .await
                .expect("publish should succeed");

        assert!(!response.publish_result.message_id.is_empty());
    }

    #[tokio::test]
    async fn publish_to_topic_with_sqs_subscriptions() {
        let sns = SnsTestStore::with_topic(topic());
        sns.create_subscription(subscription())
            .await
            .expect("setup subscription should succeed");
        let sqs = SqsTestStore::default();
        let store = service_store(sns, sqs);
        let request = resolved_request(
            "TopicArn=arn:aws:sns:us-east-1:123456789012:test-topic&Message=hello",
        );
        let body: PublishRequest = crate::actions::test_support::parse_request_body(&request);
        let trace_context = TraceContext::new("test-request-id");

        let response =
            handle_publish_typed(&request, &store, body, &trace_context, &NoopTraceRecorder)
                .await
                .expect("publish should succeed despite sqs delivery failure");

        assert!(!response.publish_result.message_id.is_empty());
    }

    #[tokio::test]
    async fn topic_not_found_error() {
        let sns = SnsTestStore::default();
        let sqs = SqsTestStore::default();
        let store = service_store(sns, sqs);
        let request = resolved_request(
            "TopicArn=arn:aws:sns:us-east-1:123456789012:test-topic&Message=hello",
        );
        let body: PublishRequest = crate::actions::test_support::parse_request_body(&request);
        let trace_context = TraceContext::new("test-request-id");

        let result =
            handle_publish_typed(&request, &store, body, &trace_context, &NoopTraceRecorder).await;
        assert!(matches!(result, Err(crate::error::SnsError::TopicNotFound)));
    }
}
