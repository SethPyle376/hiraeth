use std::collections::HashMap;

use chrono::Utc;
use hiraeth_core::{
    ResolvedRequest,
    auth::{AuthorizationCheck, Policy, PolicyPrincipal, authorize_cross_service},
    tracing::{TraceContext, TraceRecorder},
};
use hiraeth_store::sns::SnsStore;
use hiraeth_store::sqs::SqsStore;
use serde::{Deserialize, Serialize};

use crate::{
    actions::action_support::{ResponseMetadata, SNS_XMLNS},
    error::SnsError,
    store::SnsServiceStore,
};

pub(crate) struct PublishAction;

hiraeth_core::impl_aws_action! {
    PublishAction<SnsServiceStore<SS, QS>> where SS: SnsStore, QS: SqsStore {
        request: PublishRequest,
        response: PublishResponse,
        defaults: crate::SnsActionDefaults,
        name: "Publish",
        validate: |_request, payload, _store| {
            if payload.topic_arn.is_empty() {
                return Err(SnsError::BadRequest("TopicArn is required".to_string()));
            }
            if payload.message.is_empty() {
                return Err(SnsError::BadRequest("Message is required".to_string()));
            }
            Ok(())
        },
        handler: handle_publish,
        authorize: |request, _payload, store| {
            crate::auth::resolve_authorization("sns:Publish", request, &store.sns_store).await
        },
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub(crate) struct PublishRequest {
    topic_arn: String,
    message: String,
    #[serde(default)]
    subject: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "PascalCase")]
pub(crate) struct PublishResult {
    message_id: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "PascalCase")]
pub(crate) struct PublishResponse {
    #[serde(rename = "@xmlns")]
    xmlns: &'static str,
    publish_result: PublishResult,
    response_metadata: ResponseMetadata,
}

async fn handle_publish<SS, QS>(
    request: ResolvedRequest,
    payload: PublishRequest,
    store: &SnsServiceStore<SS, QS>,
    trace_context: &TraceContext,
    trace_recorder: &dyn TraceRecorder,
) -> Result<PublishResponse, SnsError>
where
    SS: SnsStore + Send + Sync,
    QS: SqsStore + Send + Sync,
{
    let attributes = HashMap::from([
        ("topic_arn".to_string(), payload.topic_arn.clone()),
        (
            "message_bytes".to_string(),
            payload.message.len().to_string(),
        ),
    ]);

    let timer = trace_context.start_span();
    let publish_context = trace_context.child_context(&timer);

    let result =
        handle_publish_typed(&request, store, payload, &publish_context, trace_recorder).await;

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

    let resolved_topic_arn = format!(
        "arn:aws:sns:{}:{}:{}",
        topic.region, topic.account_id, topic.name
    );
    let subscriptions = store
        .list_subscriptions_by_topic(&resolved_topic_arn)
        .await?;

    let subscription_timer = trace_context.start_span();
    let subscription_context = trace_context.child_context(&subscription_timer);
    subscription_context
        .record_span_or_warn(
            trace_recorder,
            subscription_timer,
            "sns.subscriptions.resolve",
            "sns",
            "ok",
            HashMap::from([
                ("topic_arn".to_string(), request_body.topic_arn.clone()),
                ("resolved_topic_arn".to_string(), resolved_topic_arn.clone()),
                (
                    "subscription_count".to_string(),
                    subscriptions.len().to_string(),
                ),
                (
                    "subscription_endpoints".to_string(),
                    subscriptions
                        .iter()
                        .map(|subscription| subscription.endpoint.as_str())
                        .collect::<Vec<_>>()
                        .join(","),
                ),
            ]),
        )
        .await;

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
        .ok_or_else(|| {
        SnsError::BadRequest(format!("Invalid SQS endpoint: {}", subscription.endpoint))
    })?;

    let sqs_queue = store
        .sqs_store
        .get_queue(&queue_id.name, &queue_id.region, &queue_id.account_id)
        .await?
        .ok_or_else(|| {
            SnsError::BadRequest(format!("SQS queue not found: {}", subscription.endpoint))
        })?;

    let queue_arn = format!(
        "arn:aws:sqs:{}:{}:{}",
        sqs_queue.region, sqs_queue.account_id, sqs_queue.name
    );

    let resource_policy = parse_sqs_resource_policy(&sqs_queue.policy)?;

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

    let delivery_date = Utc::now();
    let visible_at = delivery_date.naive_utc() + chrono::Duration::seconds(sqs_queue.delay_seconds);
    let body_bytes = body.len();

    let enqueue_result = hiraeth_sqs::operations::enqueue_message(
        &store.sqs_store,
        &sqs_queue,
        body,
        None,
        None,
        None,
        None,
        delivery_date,
    )
    .await;

    let deliver_status = if enqueue_result.is_ok() {
        "ok"
    } else {
        "error"
    };
    let mut deliver_attributes = HashMap::from([
        ("topic_arn".to_string(), request_body.topic_arn.clone()),
        ("protocol".to_string(), subscription.protocol.clone()),
        ("endpoint".to_string(), subscription.endpoint.clone()),
        ("queue_id".to_string(), sqs_queue.id.to_string()),
        ("queue_name".to_string(), sqs_queue.name.clone()),
        ("queue_region".to_string(), sqs_queue.region.clone()),
        ("queue_account_id".to_string(), sqs_queue.account_id.clone()),
        (
            "queue_delay_seconds".to_string(),
            sqs_queue.delay_seconds.to_string(),
        ),
        ("visible_at".to_string(), visible_at.to_string()),
        ("raw_message_delivery".to_string(), raw_delivery.to_string()),
        ("body_bytes".to_string(), body_bytes.to_string()),
    ]);
    match &enqueue_result {
        Ok(sqs_message_id) => {
            deliver_attributes.insert("sqs_message_id".to_string(), sqs_message_id.clone());
        }
        Err(error) => {
            deliver_attributes.insert("error".to_string(), error.to_string());
        }
    }

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

fn parse_sqs_resource_policy(policy: &str) -> Result<Policy, SnsError> {
    if policy.trim().is_empty() || policy.trim() == "{}" {
        return Ok(Policy {
            version: "2012-10-17".to_string(),
            statement: vec![],
        });
    }

    serde_json::from_str(policy)
        .map_err(|e| SnsError::InternalError(format!("invalid queue policy: {}", e)))
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
        sqs::SqsQueue,
        test_support::{SnsTestStore, SqsTestStore},
    };

    use super::{PublishAction, PublishRequest, handle_publish_typed, parse_sqs_resource_policy};
    use crate::error::SnsError;
    use crate::store::SnsServiceStore;

    fn resolved_request(body: &str) -> ResolvedRequest {
        let mut headers = HashMap::new();
        headers.insert("x-amz-target".to_string(), "AmazonSNS.Publish".to_string());

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

    fn queue() -> SqsQueue {
        SqsQueue {
            id: 7,
            name: "test-queue".to_string(),
            region: "us-east-1".to_string(),
            account_id: "123456789012".to_string(),
            queue_type: "standard".to_string(),
            visibility_timeout_seconds: 30,
            delay_seconds: 120,
            maximum_message_size: 262_144,
            message_retention_period_seconds: 345_600,
            receive_message_wait_time_seconds: 0,
            policy: "{}".to_string(),
            redrive_policy: "{}".to_string(),
            content_based_deduplication: false,
            kms_master_key_id: None,
            kms_data_key_reuse_period_seconds: 300,
            deduplication_scope: "queue".to_string(),
            fifo_throughput_limit: "perQueue".to_string(),
            redrive_allow_policy: "{}".to_string(),
            sqs_managed_sse_enabled: false,
            created_at: Utc::now().naive_utc(),
            updated_at: Utc::now().naive_utc(),
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
    async fn publish_to_topic_with_matching_sqs_subscription_enqueues_message() {
        let sns = SnsTestStore::with_topic(topic());
        sns.create_subscription(subscription())
            .await
            .expect("setup subscription should succeed");
        let sqs = SqsTestStore::with_queue(queue());
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
        let sent_messages = store.sqs_store.sent_messages();
        assert_eq!(sent_messages.len(), 1);
        assert_eq!(sent_messages[0].queue_id, 7);
        assert!(sent_messages[0].body.contains("hello"));
        assert_eq!(
            sent_messages[0].visible_at - sent_messages[0].sent_at,
            chrono::Duration::seconds(120)
        );
    }

    #[test]
    fn parse_sqs_resource_policy_rejects_statement_without_resource() {
        let result = parse_sqs_resource_policy(
            r#"{
                "Version":"2012-10-17",
                "Statement":[
                    {
                        "Effect":"Allow",
                        "Principal":{"Service":"sns.amazonaws.com"},
                        "Action":"sqs:SendMessage"
                    }
                ]
            }"#,
        );

        assert!(matches!(result, Err(SnsError::InternalError(_))));
    }

    #[test]
    fn parse_sqs_resource_policy_accepts_statement_with_resource() {
        let policy = parse_sqs_resource_policy(
            r#"{
                "Version":"2012-10-17",
                "Statement":[
                    {
                        "Effect":"Allow",
                        "Principal":{"Service":"sns.amazonaws.com"},
                        "Action":"sqs:SendMessage",
                        "Resource":"arn:aws:sqs:us-east-1:123456789012:test-queue"
                    }
                ]
            }"#,
        )
        .expect("policy should parse");

        assert_eq!(policy.statement.len(), 1);
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
