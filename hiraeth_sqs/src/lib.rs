use crate::{
    change_message_visibility::*, delete_message::*, list_queues::list_queues, queue::*,
    queue_attributes::get_queue_attributes, receive_message::*, send_message::*,
    set_queue_attributes::*, tags::*,
};
use async_trait::async_trait;
use hiraeth_auth::ResolvedRequest;
use hiraeth_core::{
    ApiError, ServiceResponse, auth::AuthorizationCheck, auth::Policy, render_result,
};
use hiraeth_router::Service;
use hiraeth_store::sqs::SqsStore;

mod auth;
mod change_message_visibility;
mod delete_message;
mod error;
mod list_queues;
mod queue;
mod queue_attributes;
mod receive_message;
mod send_message;
mod set_queue_attributes;
mod tags;
mod util;

pub struct SqsService<S: SqsStore> {
    store: S,
}

impl<S: SqsStore> SqsService<S> {
    pub fn new(store: S) -> Self {
        Self { store }
    }
}

#[async_trait]
impl<S> Service for SqsService<S>
where
    S: SqsStore + Send + Sync + 'static,
{
    fn can_handle(&self, request: &ResolvedRequest) -> bool {
        request.service == "sqs"
    }

    async fn handle_request(
        &self,
        request: ResolvedRequest,
    ) -> Result<ServiceResponse, hiraeth_core::ApiError> {
        let result = match request.request.headers.get("x-amz-target") {
            Some(target) => Ok(match target.as_str() {
                "AmazonSQS.CreateQueue" => create_queue(&request, &self.store).await,
                "AmazonSQS.DeleteQueue" => delete_queue(&request, &self.store).await,
                "AmazonSQS.PurgeQueue" => purge_queue(&request, &self.store).await,
                "AmazonSQS.ListQueues" => list_queues(&request, &self.store).await,
                "AmazonSQS.SetQueueAttributes" => set_queue_attributes(&request, &self.store).await,
                "AmazonSQS.GetQueueUrl" => get_queue_url(&request, &self.store).await,
                "AmazonSQS.SendMessage" => send_message(&request, &self.store).await,
                "AmazonSQS.SendMessageBatch" => send_message_batch(&request, &self.store).await,
                "AmazonSQS.ReceiveMessage" => receive_message(&request, &self.store).await,
                "AmazonSQS.GetQueueAttributes" => get_queue_attributes(&request, &self.store).await,
                "AmazonSQS.ListQueueTags" => list_queue_tags(&request, &self.store).await,
                "AmazonSQS.TagQueue" => tag_queue(&request, &self.store).await,
                "AmazonSQS.UntagQueue" => untag_queue(&request, &self.store).await,
                "AmazonSQS.DeleteMessage" => delete_message(&request, &self.store).await,
                "AmazonSQS.DeleteMessageBatch" => delete_message_batch(&request, &self.store).await,
                "AmazonSQS.ChangeMessageVisibility" => {
                    change_message_visibility(&request, &self.store).await
                }
                "AmazonSQS.ChangeMessageVisibilityBatch" => {
                    change_message_visibility_batch(&request, &self.store).await
                }
                op => Err(error::SqsError::UnsupportedOperation(op.to_string())),
            }),
            _ => Err(ApiError::NotFound(
                "Missing x-amz-target header".to_string(),
            )),
        };
        result.map(render_result)
    }

    async fn auth_request(
        &self,
        request: &ResolvedRequest,
    ) -> Result<AuthorizationCheck, ServiceResponse> {
        let action = auth::get_action_for_request(request).map_err(ServiceResponse::from)?;
        let relevant_queue = auth::get_relevant_queue_for_action(&action, request, &self.store)
            .await
            .map_err(ServiceResponse::from)?;

        let resource = relevant_queue
            .as_ref()
            .map(util::get_queue_arn)
            .unwrap_or_else(|| "*".to_string());

        let policy = relevant_queue
            .map(|queue| queue.policy.clone())
            .map(|policy| {
                serde_json::from_str::<Policy>(&policy).unwrap_or_else(|_| Policy::default())
            });

        Ok(AuthorizationCheck {
            action,
            resource,
            resource_policy: policy,
        })
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use chrono::{TimeZone, Utc};
    use hiraeth_auth::{AuthContext, ResolvedRequest};
    use hiraeth_http::IncomingRequest;
    use hiraeth_store::{principal::Principal, sqs::SqsQueue, test_support::SqsTestStore};
    use serde_json::Value;

    use super::{Service, ServiceResponse, SqsService, queue};
    use crate::error::SqsError;

    fn resolved_request(target: Option<&str>, body: &str) -> ResolvedRequest {
        let mut headers = HashMap::new();
        if let Some(target) = target {
            headers.insert("x-amz-target".to_string(), target.to_string());
        }

        ResolvedRequest {
            request: IncomingRequest {
                host: "sqs.us-east-1.amazonaws.com".to_string(),
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
                        .with_ymd_and_hms(2026, 4, 1, 12, 0, 0)
                        .unwrap()
                        .naive_utc(),
                },
            },
            date: Utc.with_ymd_and_hms(2026, 4, 1, 12, 0, 0).unwrap(),
        }
    }

    fn parse_json_body(response: &ServiceResponse) -> Value {
        serde_json::from_slice(&response.body).expect("response body should be valid json")
    }

    #[tokio::test]
    async fn create_queue_persists_defaults_and_returns_queue_url() {
        let store = SqsTestStore::default();
        let request = resolved_request(
            Some("AmazonSQS.CreateQueue"),
            r#"{"QueueName":"test-queue"}"#,
        );

        let response = queue::create_queue(&request, &store)
            .await
            .expect("create queue should succeed");

        assert_eq!(response.status_code, 200);
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
            Some("AmazonSQS.CreateQueue"),
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

        queue::create_queue(&request, &store)
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

    #[tokio::test]
    async fn get_queue_url_returns_not_found_when_queue_does_not_exist() {
        let store = SqsTestStore::default();
        let request = resolved_request(
            Some("AmazonSQS.GetQueueUrl"),
            r#"{"QueueName":"missing-queue"}"#,
        );

        let result = queue::get_queue_url(&request, &store).await;

        assert!(matches!(result, Err(SqsError::QueueNotFound)));
    }

    #[tokio::test]
    async fn get_queue_url_returns_queue_url_when_queue_exists() {
        let store = SqsTestStore::with_queue(SqsQueue {
            id: 1,
            name: "existing-queue".to_string(),
            region: "us-east-1".to_string(),
            account_id: "123456789012".to_string(),
            queue_type: "standard".to_string(),
            visibility_timeout_seconds: 30,
            delay_seconds: 0,
            message_retention_period_seconds: 345600,
            receive_message_wait_time_seconds: 0,
            created_at: Utc
                .with_ymd_and_hms(2026, 4, 1, 12, 0, 0)
                .unwrap()
                .naive_utc(),
            updated_at: Utc
                .with_ymd_and_hms(2026, 4, 1, 12, 0, 0)
                .unwrap()
                .naive_utc(),
            ..Default::default()
        });
        let request = resolved_request(
            Some("AmazonSQS.GetQueueUrl"),
            r#"{"QueueName":"existing-queue"}"#,
        );

        let response = queue::get_queue_url(&request, &store)
            .await
            .expect("get queue url should succeed");

        assert_eq!(response.status_code, 200);
        assert_eq!(
            parse_json_body(&response)["QueueUrl"],
            "http://sqs.us-east-1.amazonaws.com/123456789012/existing-queue"
        );
    }

    #[tokio::test]
    async fn auth_request_returns_action_and_resource_for_queue_action() {
        let service = SqsService::new(SqsTestStore::with_queue(SqsQueue {
            id: 1,
            name: "existing-queue".to_string(),
            region: "us-east-1".to_string(),
            account_id: "123456789012".to_string(),
            ..Default::default()
        }));
        let request = resolved_request(
            Some("AmazonSQS.SendMessage"),
            r#"{
                "QueueUrl":"http://sqs.us-east-1.amazonaws.com/123456789012/existing-queue",
                "MessageBody":"hello"
            }"#,
        );

        let check = service
            .auth_request(&request)
            .await
            .expect("auth check should resolve queue context");

        assert_eq!(check.action, "sqs:SendMessage");
        assert_eq!(
            check.resource,
            "arn:aws:sqs:us-east-1:123456789012:existing-queue"
        );
        assert!(check.resource_policy.is_some());
    }

    #[tokio::test]
    async fn auth_request_renders_sqs_error_response_for_queue_lookup_failure() {
        let service = SqsService::new(SqsTestStore::default());
        let request = resolved_request(
            Some("AmazonSQS.SendMessage"),
            r#"{
                "QueueUrl":"http://sqs.us-east-1.amazonaws.com/123456789012/missing-queue",
                "MessageBody":"hello"
            }"#,
        );

        let response = service
            .auth_request(&request)
            .await
            .expect_err("queue lookup failures should render as SQS responses");

        assert_eq!(response.status_code, 400);
        assert_eq!(
            response
                .headers
                .iter()
                .find(|(name, _)| name == "x-amzn-query-error")
                .map(|(_, value)| value.as_str()),
            Some("AWS.SimpleQueueService.NonExistentQueue;Sender")
        );

        let body = parse_json_body(&response);
        assert_eq!(body["__type"], "com.amazonaws.sqs#QueueDoesNotExist");
        assert_eq!(body["message"], "The specified queue does not exist.");
    }

    #[tokio::test]
    async fn service_returns_not_found_for_missing_target_header() {
        let service = SqsService::new(SqsTestStore::default());
        let request = resolved_request(None, r#"{"QueueName":"test-queue"}"#);

        let result = service.handle_request(request).await;

        assert!(matches!(
            result,
            Err(hiraeth_core::ApiError::NotFound(message))
                if message == "Missing x-amz-target header"
        ));
    }

    #[tokio::test]
    async fn service_returns_not_found_for_unknown_action() {
        let service = SqsService::new(SqsTestStore::default());
        let request = resolved_request(Some("AmazonSQS.DoesNotExist"), "{}");

        let response = service
            .handle_request(request)
            .await
            .expect("unknown SQS action should render an SQS error response");

        assert_eq!(response.status_code, 400);
        assert_eq!(
            response
                .headers
                .iter()
                .find(|(name, _)| name == "x-amzn-query-error")
                .map(|(_, value)| value.as_str()),
            Some("AWS.SimpleQueueService.UnsupportedOperation;Sender")
        );

        let body = parse_json_body(&response);
        assert_eq!(body["__type"], "com.amazonaws.sqs#UnsupportedOperation");
        assert_eq!(body["message"], "AmazonSQS.DoesNotExist");
    }

    #[tokio::test]
    async fn service_renders_queue_not_found_as_sqs_error_response() {
        let service = SqsService::new(SqsTestStore::default());
        let request = resolved_request(
            Some("AmazonSQS.GetQueueUrl"),
            r#"{"QueueName":"missing-queue"}"#,
        );

        let response = service
            .handle_request(request)
            .await
            .expect("service should render SQS errors as a response");

        assert_eq!(response.status_code, 400);
        assert_eq!(
            response
                .headers
                .iter()
                .find(|(name, _)| name == "content-type")
                .map(|(_, value)| value.as_str()),
            Some("application/x-amz-json-1.0")
        );
        assert_eq!(
            response
                .headers
                .iter()
                .find(|(name, _)| name == "x-amzn-query-error")
                .map(|(_, value)| value.as_str()),
            Some("AWS.SimpleQueueService.NonExistentQueue;Sender")
        );

        let body = parse_json_body(&response);
        assert_eq!(body["__type"], "com.amazonaws.sqs#QueueDoesNotExist");
        assert_eq!(body["message"], "The specified queue does not exist.");
    }
}
