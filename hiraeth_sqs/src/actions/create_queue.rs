use std::collections::HashMap;

use async_trait::async_trait;
use hiraeth_core::{
    AwsActionPayloadFormat, AwsActionPayloadParseError, ResolvedRequest, TypedAwsAction,
    auth::AuthorizationCheck,
    tracing::{TraceContext, TraceRecorder},
};
use hiraeth_store::{
    StoreError,
    sqs::{SqsQueue, SqsStore},
};
use serde::{Deserialize, Serialize};

use super::{
    action_support::{json_payload_format, parse_payload_error},
    queue_attribute_support::QueueAttributeValues,
    queue_support,
    tag_support::validate_tags,
};
use crate::error::SqsError;

pub(crate) struct CreateQueueAction;

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub(crate) struct CreateQueueRequest {
    queue_name: String,
    #[serde(default)]
    attributes: HashMap<String, String>,
    #[serde(default)]
    tags: HashMap<String, String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "PascalCase")]
pub(crate) struct CreateQueueResponse {
    queue_url: String,
}

async fn handle_create_queue_typed<S: SqsStore>(
    request: &ResolvedRequest,
    store: &S,
    request_body: CreateQueueRequest,
) -> Result<CreateQueueResponse, SqsError> {
    let queue_attributes = QueueAttributeValues::from_attribute_map(&request_body.attributes)?;
    queue_support::validate_queue_name(&request_body.queue_name, queue_attributes.fifo_queue)?;
    validate_tags(&request_body.tags, true)?;

    let now = chrono::Utc::now().naive_utc();
    let queue = SqsQueue {
        id: 0,
        name: request_body.queue_name.clone(),
        region: request.region.clone(),
        account_id: request.auth_context.principal.account_id.clone(),
        queue_type: if queue_attributes.fifo_queue {
            "fifo".to_string()
        } else {
            "standard".to_string()
        },
        visibility_timeout_seconds: queue_attributes.visibility_timeout_seconds,
        delay_seconds: queue_attributes.delay_seconds,
        maximum_message_size: queue_attributes.maximum_message_size,
        message_retention_period_seconds: queue_attributes.message_retention_period_seconds,
        receive_message_wait_time_seconds: queue_attributes.receive_message_wait_time_seconds,
        policy: queue_attributes.policy,
        redrive_policy: queue_attributes.redrive_policy,
        content_based_deduplication: queue_attributes.content_based_deduplication,
        kms_master_key_id: queue_attributes.kms_master_key_id,
        kms_data_key_reuse_period_seconds: queue_attributes.kms_data_key_reuse_period_seconds,
        deduplication_scope: queue_attributes.deduplication_scope,
        fifo_throughput_limit: queue_attributes.fifo_throughput_limit,
        redrive_allow_policy: queue_attributes.redrive_allow_policy,
        sqs_managed_sse_enabled: queue_attributes.sqs_managed_sse_enabled,
        created_at: now,
        updated_at: now,
    };

    match store.create_queue(queue.clone()).await {
        Ok(()) => {
            if !request_body.tags.is_empty() {
                let created_queue = store
                    .get_queue(
                        &request_body.queue_name,
                        &request.region,
                        &request.auth_context.principal.account_id,
                    )
                    .await
                    .map_err(|e| SqsError::InternalError(e.to_string()))?
                    .ok_or_else(|| {
                        SqsError::InternalError(
                            "created queue could not be loaded for tagging".to_string(),
                        )
                    })?;

                store
                    .tag_queue(created_queue.id, request_body.tags)
                    .await
                    .map_err(crate::error::map_store_error)?;
            }

            create_queue_response(
                request,
                &request.auth_context.principal.account_id,
                &request_body.queue_name,
            )
        }
        Err(StoreError::Conflict(_)) => {
            let existing_queue = store
                .get_queue(
                    &request_body.queue_name,
                    &request.region,
                    &request.auth_context.principal.account_id,
                )
                .await
                .map_err(|e| SqsError::InternalError(e.to_string()))?;

            match existing_queue {
                Some(existing_queue)
                    if queue_support::queue_configuration_matches(&existing_queue, &queue) =>
                {
                    create_queue_response(
                        request,
                        &request.auth_context.principal.account_id,
                        &request_body.queue_name,
                    )
                }
                Some(_) => Err(SqsError::QueueAlreadyExists(format!(
                    "A queue named '{}' already exists with different attributes.",
                    request_body.queue_name
                ))),
                None => Err(SqsError::InternalError(
                    "queue creation conflicted but existing queue could not be loaded".to_string(),
                )),
            }
        }
        Err(e) => Err(SqsError::InternalError(e.to_string())),
    }
}

fn create_queue_response(
    request: &ResolvedRequest,
    account_id: &str,
    queue_name: &str,
) -> Result<CreateQueueResponse, SqsError> {
    Ok(CreateQueueResponse {
        queue_url: crate::util::queue_url(&request.request.host, account_id, queue_name),
    })
}

#[async_trait]
impl<S> TypedAwsAction<S> for CreateQueueAction
where
    S: SqsStore + Send + Sync,
{
    type Request = CreateQueueRequest;
    type Response = CreateQueueResponse;
    type Error = SqsError;

    fn name(&self) -> &'static str {
        "CreateQueue"
    }

    fn payload_format(&self) -> AwsActionPayloadFormat {
        json_payload_format()
    }

    fn parse_error(&self, error: AwsActionPayloadParseError) -> SqsError {
        parse_payload_error(error)
    }

    async fn validate(
        &self,
        _request: &ResolvedRequest,
        request_body: &CreateQueueRequest,
        _store: &S,
    ) -> Result<(), SqsError> {
        let queue_attributes = QueueAttributeValues::from_attribute_map(&request_body.attributes)?;
        queue_support::validate_queue_name(&request_body.queue_name, queue_attributes.fifo_queue)?;
        validate_tags(&request_body.tags, true)?;
        Ok(())
    }

    async fn handle(
        &self,
        request: ResolvedRequest,
        request_body: CreateQueueRequest,
        store: &S,
        trace_context: &TraceContext,
        trace_recorder: &dyn TraceRecorder,
    ) -> Result<CreateQueueResponse, SqsError> {
        let attributes = HashMap::from([
            ("queue_name".to_string(), request_body.queue_name.clone()),
            ("region".to_string(), request.region.clone()),
            (
                "account_id".to_string(),
                request.auth_context.principal.account_id.clone(),
            ),
            (
                "requested_attribute_count".to_string(),
                request_body.attributes.len().to_string(),
            ),
            ("tag_count".to_string(), request_body.tags.len().to_string()),
            (
                "fifo_queue".to_string(),
                request_body
                    .attributes
                    .get("FifoQueue")
                    .map(String::as_str)
                    .unwrap_or("false")
                    .to_string(),
            ),
        ]);

        trace_context
            .record_result_span(
                trace_recorder,
                "sqs.queue.create",
                "sqs",
                attributes,
                async { handle_create_queue_typed(&request, store, request_body).await },
            )
            .await
    }

    async fn resolve_authorization_typed(
        &self,
        request: &ResolvedRequest,
        _payload: CreateQueueRequest,
        store: &S,
    ) -> Result<AuthorizationCheck, SqsError> {
        crate::auth::resolve_authorization("sqs:CreateQueue", request, store).await
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use chrono::{TimeZone, Utc};
    use hiraeth_core::{AuthContext, ResolvedRequest, TypedAwsAction};
    use hiraeth_http::IncomingRequest;
    use hiraeth_router::ServiceResponse;
    use hiraeth_store::{principal::Principal, test_support::SqsTestStore};
    use serde_json::Value;

    use super::{CreateQueueAction, handle_create_queue_typed};

    fn resolved_request(body: &str) -> ResolvedRequest {
        let mut headers = HashMap::new();
        headers.insert(
            "x-amz-target".to_string(),
            "AmazonSQS.CreateQueue".to_string(),
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
            service: "sqs".to_string(),
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

    fn aws_style_resolved_request(body: &str) -> ResolvedRequest {
        let mut request = resolved_request(body);
        request.request.host = "sqs.us-east-1.amazonaws.com".to_string();
        request
    }

    fn parse_json_body<T: serde::Serialize>(response: &T) -> Value {
        serde_json::to_value(response).expect("response should serialize to json")
    }

    #[test]
    fn reports_expected_action_name() {
        assert_eq!(
            <CreateQueueAction as TypedAwsAction<SqsTestStore>>::name(&CreateQueueAction),
            "CreateQueue"
        );
    }

    #[tokio::test]
    async fn create_queue_persists_supplied_tags() {
        let store = SqsTestStore::default();
        let request = resolved_request(
            r#"{
                "QueueName":"orders",
                "Tags":{
                    "environment":"test",
                    "owner":"hiraeth"
                }
            }"#,
        );

        let response = handle_create_queue_typed(
            &request,
            &store,
            crate::actions::test_support::parse_request_body(&request),
        )
        .await
        .expect("create queue should succeed");
        assert_eq!(
            store.queue_tags(0),
            [
                ("environment".to_string(), "test".to_string()),
                ("owner".to_string(), "hiraeth".to_string()),
            ]
            .into_iter()
            .collect::<HashMap<_, _>>()
        );
    }

    #[tokio::test]
    async fn create_queue_persists_defaults_and_returns_queue_url() {
        let store = SqsTestStore::default();
        let request = aws_style_resolved_request(r#"{"QueueName":"test-queue"}"#);

        let response = handle_create_queue_typed(
            &request,
            &store,
            crate::actions::test_support::parse_request_body(&request),
        )
        .await
        .expect("create queue should succeed");
        assert_eq!(
            parse_json_body(&response)["QueueUrl"],
            "http://sqs.us-east-1.amazonaws.com/123456789012/test-queue"
        );

        let created = store.created_queues();
        assert_eq!(created.len(), 1);
        assert_eq!(created[0].name, "test-queue");
        assert_eq!(created[0].region, "us-east-1");
        assert_eq!(created[0].account_id, "123456789012");
        assert_eq!(created[0].queue_type, "standard");
        assert_eq!(created[0].visibility_timeout_seconds, 30);
        assert_eq!(created[0].delay_seconds, 0);
        assert_eq!(created[0].maximum_message_size, 1048576);
        assert_eq!(created[0].message_retention_period_seconds, 345600);
        assert_eq!(created[0].receive_message_wait_time_seconds, 0);
        assert_eq!(created[0].policy, "{}");
        assert_eq!(created[0].redrive_policy, "{}");
        assert!(!created[0].content_based_deduplication);
        assert_eq!(created[0].kms_master_key_id, None);
        assert_eq!(created[0].kms_data_key_reuse_period_seconds, 300);
        assert_eq!(created[0].deduplication_scope, "queue");
        assert_eq!(created[0].fifo_throughput_limit, "perQueue");
        assert_eq!(created[0].redrive_allow_policy, "{}");
        assert!(!created[0].sqs_managed_sse_enabled);
    }

    #[tokio::test]
    async fn create_queue_uses_supplied_attribute_values() {
        let store = SqsTestStore::default();
        let request = resolved_request(
            r#"{
                "QueueName":"configured-queue.fifo",
                "Attributes":{
                    "VisibilityTimeout":"45",
                    "DelaySeconds":"5",
                    "MaximumMessageSize":"2048",
                    "MessageRetentionPeriod":"86400",
                    "ReceiveMessageWaitTimeSeconds":"10",
                    "Policy":"{\"Statement\":[]}",
                    "RedrivePolicy":"{\"maxReceiveCount\":\"5\"}",
                    "FifoQueue":"true",
                    "ContentBasedDeduplication":"true",
                    "KmsMasterKeyId":"alias/test",
                    "KmsDataKeyReusePeriodSeconds":"600",
                    "DeduplicationScope":"messageGroup",
                    "FifoThroughputLimit":"perMessageGroupId",
                    "RedriveAllowPolicy":"{\"redrivePermission\":\"allowAll\"}",
                    "SqsManagedSseEnabled":"true"
                }
            }"#,
        );

        handle_create_queue_typed(
            &request,
            &store,
            crate::actions::test_support::parse_request_body(&request),
        )
        .await
        .expect("create queue should succeed");

        let created = store.created_queues();
        assert_eq!(created.len(), 1);
        assert_eq!(created[0].queue_type, "fifo");
        assert_eq!(created[0].visibility_timeout_seconds, 45);
        assert_eq!(created[0].delay_seconds, 5);
        assert_eq!(created[0].maximum_message_size, 2048);
        assert_eq!(created[0].message_retention_period_seconds, 86400);
        assert_eq!(created[0].receive_message_wait_time_seconds, 10);
        assert_eq!(created[0].policy, r#"{"Statement":[]}"#);
        assert_eq!(created[0].redrive_policy, r#"{"maxReceiveCount":"5"}"#);
        assert!(created[0].content_based_deduplication);
        assert_eq!(created[0].kms_master_key_id.as_deref(), Some("alias/test"));
        assert_eq!(created[0].kms_data_key_reuse_period_seconds, 600);
        assert_eq!(created[0].deduplication_scope, "messageGroup");
        assert_eq!(created[0].fifo_throughput_limit, "perMessageGroupId");
        assert_eq!(
            created[0].redrive_allow_policy,
            r#"{"redrivePermission":"allowAll"}"#
        );
        assert!(created[0].sqs_managed_sse_enabled);
    }
}
