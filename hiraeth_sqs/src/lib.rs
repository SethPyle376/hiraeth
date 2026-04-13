use crate::{
    change_message_visibility::*, delete_message::*, error::render_result,
    list_queues::list_queues, queue::*, queue_attributes::get_queue_attributes, receive_message::*,
    send_message::*,
};
use async_trait::async_trait;
use hiraeth_auth::ResolvedRequest;
use hiraeth_core::ApiError;
use hiraeth_router::{Service, ServiceResponse};
use hiraeth_store::sqs::SqsStore;

mod change_message_visibility;
mod delete_message;
mod error;
mod list_queues;
mod queue;
mod queue_attributes;
mod receive_message;
mod send_message;
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
                "AmazonSQS.ListQueues" => list_queues(&request, &self.store).await,
                "AmazonSQS.GetQueueUrl" => get_queue_url(&request, &self.store).await,
                "AmazonSQS.SendMessage" => send_message(&request, &self.store).await,
                "AmazonSQS.SendMessageBatch" => send_message_batch(&request, &self.store).await,
                "AmazonSQS.ReceiveMessage" => receive_message(&request, &self.store).await,
                "AmazonSQS.GetQueueAttributes" => get_queue_attributes(&request, &self.store).await,
                "AmazonSQS.DeleteMessage" => delete_message(&request, &self.store).await,
                "AmazonSQS.DeleteMessageBatch" => delete_message_batch(&request, &self.store).await,
                "AmazonSQS.ChangeMessageVisibility" => {
                    change_message_visibility(&request, &self.store).await
                }
                op => Err(error::SqsError::UnsupportedOperation(op.to_string())),
            }),
            _ => Err(ApiError::NotFound(
                "Missing x-amz-target header".to_string(),
            )),
        };

        result.map(|op_result| render_result(op_result))
    }
}

#[cfg(test)]
mod tests {
    use std::{
        collections::HashMap,
        sync::{Mutex, MutexGuard},
    };

    use chrono::{TimeZone, Utc};
    use hiraeth_auth::{AuthContext, ResolvedRequest};
    use hiraeth_http::IncomingRequest;
    use hiraeth_store::{
        StoreError,
        principal::Principal,
        sqs::{SqsQueue, SqsStore},
    };
    use serde_json::Value;

    use super::{Service, ServiceResponse, SqsService, queue};
    use crate::error::SqsError;

    #[derive(Default)]
    struct TestSqsStore {
        queues: Mutex<HashMap<(String, String), SqsQueue>>,
        created_queues: Mutex<Vec<SqsQueue>>,
    }

    impl TestSqsStore {
        fn with_queue(queue: SqsQueue) -> Self {
            let mut queues = HashMap::new();
            queues.insert((queue.name.clone(), queue.region.clone()), queue);

            Self {
                queues: Mutex::new(queues),
                created_queues: Mutex::new(Vec::new()),
            }
        }

        fn created_queues(&self) -> MutexGuard<'_, Vec<SqsQueue>> {
            self.created_queues.lock().expect("created queues mutex")
        }
    }

    #[async_trait::async_trait]
    impl SqsStore for TestSqsStore {
        async fn create_queue(&self, queue: SqsQueue) -> Result<(), StoreError> {
            self.queues
                .lock()
                .expect("queues mutex")
                .insert((queue.name.clone(), queue.region.clone()), queue.clone());
            self.created_queues
                .lock()
                .expect("created queues mutex")
                .push(queue);
            Ok(())
        }

        async fn delete_queue(&self, _queue_id: i64) -> Result<(), StoreError> {
            unimplemented!()
        }

        async fn get_queue(
            &self,
            queue_name: &str,
            region: &str,
            _account_id: &str,
        ) -> Result<Option<SqsQueue>, StoreError> {
            Ok(self
                .queues
                .lock()
                .expect("queues mutex")
                .get(&(queue_name.to_string(), region.to_string()))
                .cloned())
        }

        async fn get_message_count(&self, _queue_id: i64) -> Result<i64, StoreError> {
            unimplemented!()
        }

        async fn get_visible_message_count(&self, _queue_id: i64) -> Result<i64, StoreError> {
            unimplemented!()
        }

        async fn get_messages_delayed_count(&self, _queue_id: i64) -> Result<i64, StoreError> {
            unimplemented!()
        }

        async fn list_queues(
            &self,
            _region: &str,
            _account_id: &str,
            _queue_name_prefix: Option<&str>,
            _max_results: Option<i64>,
            _next_token: Option<&str>,
        ) -> Result<Vec<SqsQueue>, StoreError> {
            unimplemented!()
        }

        async fn send_message(
            &self,
            _message: &hiraeth_store::sqs::SqsMessage,
        ) -> Result<(), StoreError> {
            unimplemented!()
        }

        async fn receive_messages(
            &self,
            _queue_id: i64,
            _max_number_of_messages: i64,
            _visibility_timeout_seconds: u32,
        ) -> Result<Vec<hiraeth_store::sqs::SqsMessage>, StoreError> {
            unimplemented!()
        }

        async fn delete_message(
            &self,
            _queue_id: i64,
            _receipt_handle: &str,
        ) -> Result<(), StoreError> {
            unimplemented!()
        }

        async fn set_message_visible_at(
            &self,
            _queue_id: i64,
            _receipt_handle: &str,
            _visible_at: chrono::NaiveDateTime,
        ) -> Result<(), StoreError> {
            unimplemented!()
        }
    }

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
        let store = TestSqsStore::default();
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
        assert_eq!(created[0].message_retention_period_seconds, 345600);
        assert_eq!(created[0].receive_message_wait_time_seconds, 0);
    }

    #[tokio::test]
    async fn create_queue_uses_supplied_attribute_values() {
        let store = TestSqsStore::default();
        let request = resolved_request(
            Some("AmazonSQS.CreateQueue"),
            r#"{
                "QueueName":"configured-queue",
                "Attributes":{
                    "VisibilityTimeout":"45",
                    "DelaySeconds":"5",
                    "MessageRetentionPeriod":"86400",
                    "ReceiveMessageWaitTimeSeconds":"10"
                }
            }"#,
        );

        queue::create_queue(&request, &store)
            .await
            .expect("create queue should succeed");

        let created = store.created_queues();
        assert_eq!(created.len(), 1);
        assert_eq!(created[0].visibility_timeout_seconds, 45);
        assert_eq!(created[0].delay_seconds, 5);
        assert_eq!(created[0].message_retention_period_seconds, 86400);
        assert_eq!(created[0].receive_message_wait_time_seconds, 10);
    }

    #[tokio::test]
    async fn get_queue_url_returns_not_found_when_queue_does_not_exist() {
        let store = TestSqsStore::default();
        let request = resolved_request(
            Some("AmazonSQS.GetQueueUrl"),
            r#"{"QueueName":"missing-queue"}"#,
        );

        let result = queue::get_queue_url(&request, &store).await;

        assert!(matches!(result, Err(SqsError::QueueNotFound)));
    }

    #[tokio::test]
    async fn get_queue_url_returns_queue_url_when_queue_exists() {
        let store = TestSqsStore::with_queue(SqsQueue {
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
    async fn service_returns_not_found_for_missing_target_header() {
        let service = SqsService::new(TestSqsStore::default());
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
        let service = SqsService::new(TestSqsStore::default());
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
        let service = SqsService::new(TestSqsStore::default());
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
