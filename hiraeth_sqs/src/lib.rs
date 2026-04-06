use async_trait::async_trait;
use hiraeth_auth::ResolvedRequest;
use hiraeth_core::ApiError;
use hiraeth_router::{Service, ServiceResponse};
use hiraeth_store::{StoreError, sqs::SqsStore};
use serde::Serialize;

mod change_message_visibility;
mod delete_message;
mod queue;
mod queue_attributes;
mod receive_message;
mod send_message;
mod util;

#[derive(Debug, Clone, PartialEq, Eq)]
enum SqsError {
    QueueNotFound(String, String, String),
    StoreError(StoreError),
    BadRequest(String),
}

#[derive(Debug, Clone, Serialize)]
struct SqsErrorBody {
    #[serde(rename = "__type")]
    error_type: String,
    message: String,
}

impl SqsError {
    fn into_response(self) -> ServiceResponse {
        match self {
            SqsError::QueueNotFound(_, _, _) => sqs_error_response(
                400,
                "AWS.SimpleQueueService.NonExistentQueue;Sender",
                "com.amazonaws.sqs#QueueDoesNotExist",
                "The specified queue does not exist.",
            ),
            SqsError::BadRequest(message) => sqs_error_response(
                400,
                "AWS.SimpleQueueService.InvalidParameterValue;Sender",
                "com.amazonaws.sqs#InvalidParameterValue",
                &message,
            ),
            SqsError::StoreError(error) => sqs_error_response(
                500,
                "AWS.SimpleQueueService.InternalError;Receiver",
                "com.amazonaws.sqs#InternalError",
                &format!("Internal SQS store error: {:?}", error),
            ),
        }
    }
}

fn sqs_error_response(
    status_code: u16,
    query_error: &str,
    error_type: &str,
    message: &str,
) -> ServiceResponse {
    let request_id = uuid::Uuid::new_v4().to_string();
    let body = serde_json::to_vec(&SqsErrorBody {
        error_type: error_type.to_string(),
        message: message.to_string(),
    })
    .unwrap_or_default();

    ServiceResponse {
        status_code,
        headers: vec![
            (
                "content-type".to_string(),
                "application/x-amz-json-1.0".to_string(),
            ),
            ("x-amzn-requestid".to_string(), request_id),
            ("x-amzn-query-error".to_string(), query_error.to_string()),
        ],
        body,
    }
}

impl From<SqsError> for ApiError {
    fn from(value: SqsError) -> ApiError {
        match value {
            SqsError::QueueNotFound(name, region, account) => ApiError::BadRequest(format!(
                "Queue not found: Name: {}, Region: {}, Account: {}",
                name, region, account
            )),
            SqsError::StoreError(sqs_store_error) => {
                ApiError::InternalServerError(format!("SQS store error: {:?}", sqs_store_error))
            }
            SqsError::BadRequest(error) => {
                ApiError::BadRequest(format!("SQS Bad Request: {:?}", error))
            }
        }
    }
}

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
        match request.request.headers.get("x-amz-target") {
            Some(target) => match target.as_str() {
                "AmazonSQS.CreateQueue" => queue::create_queue(&request, &self.store)
                    .await
                    .or_else(|error| Ok(error.into_response())),
                "AmazonSQS.DeleteQueue" => queue::delete_queue(&request, &self.store)
                    .await
                    .or_else(|error| Ok(error.into_response())),
                "AmazonSQS.GetQueueUrl" => queue::get_queue_url(&request, &self.store)
                    .await
                    .or_else(|error| Ok(error.into_response())),
                "AmazonSQS.SendMessage" => send_message::send_message(&request, &self.store)
                    .await
                    .or_else(|error| Ok(error.into_response())),
                "AmazonSQS.SendMessageBatch" => {
                    send_message::send_message_batch(&request, &self.store)
                        .await
                        .or_else(|error| Ok(error.into_response()))
                }
                "AmazonSQS.ReceiveMessage" => {
                    receive_message::receive_message(&request, &self.store)
                        .await
                        .or_else(|error| Ok(error.into_response()))
                }
                "AmazonSQS.GetQueueAttributes" => {
                    queue_attributes::get_queue_attributes(&request, &self.store)
                        .await
                        .or_else(|error| Ok(error.into_response()))
                }
                "AmazonSQS.DeleteMessage" => delete_message::delete_message(&request, &self.store)
                    .await
                    .or_else(|error| Ok(error.into_response())),
                "AmazonSQS.DeleteMessageBatch" => {
                    delete_message::delete_message_batch(&request, &self.store)
                        .await
                        .or_else(|error| Ok(error.into_response()))
                }
                "AmazonSQS.ChangeMessageVisibility" => {
                    change_message_visibility::change_message_visibility(&request, &self.store)
                        .await
                        .or_else(|error| Ok(error.into_response()))
                }
                _ => {
                    return Err(ApiError::NotFound(format!(
                        "Unknown SQS action: {}",
                        target
                    )));
                }
            },
            _ => Err(ApiError::NotFound(
                "Missing x-amz-target header".to_string(),
            )),
        }
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

    use super::{Service, ServiceResponse, SqsError, SqsService, queue};

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

        assert!(matches!(result, Err(SqsError::QueueNotFound(_, _, _))));
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

        let result = service.handle_request(request).await;

        assert!(matches!(
            result,
            Err(hiraeth_core::ApiError::NotFound(message))
                if message == "Unknown SQS action: AmazonSQS.DoesNotExist"
        ));
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
