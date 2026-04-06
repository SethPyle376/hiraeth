use std::collections::HashMap;

use hiraeth_auth::ResolvedRequest;
use hiraeth_router::ServiceResponse;
use hiraeth_store::sqs::{SqsQueue, SqsStore};
use serde::{Deserialize, Serialize};

use crate::{SqsError, util};

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct CreateQueueRequest {
    queue_name: String,
    #[serde(default)]
    attributes: HashMap<String, String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "PascalCase")]
struct CreateQueueResponse {
    queue_url: String,
}

pub(crate) async fn create_queue<S: SqsStore>(
    request: &ResolvedRequest,
    store: &S,
) -> Result<ServiceResponse, SqsError> {
    let request_body = serde_json::from_str::<CreateQueueRequest>(
        String::from_utf8(request.request.body.clone())
            .map_err(|e| SqsError::BadRequest(e.to_string()))?
            .as_str(),
    )
    .map_err(|e| SqsError::BadRequest(e.to_string()))?;

    let queue = SqsQueue {
        id: 0,
        name: request_body.queue_name.clone(),
        region: request.region.clone(),
        account_id: request.auth_context.principal.account_id.clone(),
        queue_type: "standard".to_string(),
        visibility_timeout_seconds: request_body
            .attributes
            .get("VisibilityTimeout")
            .and_then(|v| v.parse::<i64>().ok())
            .unwrap_or(30),
        delay_seconds: request_body
            .attributes
            .get("DelaySeconds")
            .and_then(|v| v.parse::<i64>().ok())
            .unwrap_or(0),
        message_retention_period_seconds: request_body
            .attributes
            .get("MessageRetentionPeriod")
            .and_then(|v| v.parse::<i64>().ok())
            .unwrap_or(345600),
        receive_message_wait_time_seconds: request_body
            .attributes
            .get("ReceiveMessageWaitTimeSeconds")
            .and_then(|v| v.parse::<i64>().ok())
            .unwrap_or(0),
        created_at: chrono::Utc::now().naive_utc(),
    };

    store
        .create_queue(queue)
        .await
        .map(|_| {
            let response = CreateQueueResponse {
                queue_url: format!(
                    "http://{}/{}/{}",
                    request.request.host,
                    request.auth_context.principal.account_id.clone(),
                    request_body.queue_name
                ),
            };
            ServiceResponse {
                status_code: 200,
                headers: vec![],
                body: serde_json::to_vec(&response).unwrap_or_default(),
            }
        })
        .map_err(|e| SqsError::StoreError(e))
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct DeleteQueueRequest {
    queue_url: String,
}

pub(crate) async fn delete_queue<S: SqsStore>(
    request: &ResolvedRequest,
    store: &S,
) -> Result<ServiceResponse, SqsError> {
    let request_body = serde_json::from_str::<DeleteQueueRequest>(
        String::from_utf8(request.request.body.clone())
            .map_err(|e| SqsError::BadRequest(e.to_string()))?
            .as_str(),
    )
    .map_err(|e| SqsError::BadRequest(e.to_string()))?;

    let queue_id = util::parse_queue_url(&request_body.queue_url, &request.region)
        .ok_or_else(|| SqsError::BadRequest("Invalid queue url".to_string()))?;

    let queue = store
        .get_queue(&queue_id.name, &queue_id.region, &queue_id.account_id)
        .await
        .map_err(|e| SqsError::StoreError(e))?
        .ok_or_else(|| {
            SqsError::QueueNotFound(
                queue_id.name.clone(),
                queue_id.region.clone(),
                queue_id.account_id.clone(),
            )
        })?;

    store
        .delete_queue(queue.id)
        .await
        .map(|_| ServiceResponse {
            status_code: 200,
            headers: vec![],
            body: vec![],
        })
        .map_err(|e| SqsError::StoreError(e))
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct GetQueueUrlRequest {
    queue_name: String,
    queue_owner_aws_account_id: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "PascalCase")]
struct GetQueueUrlResponse {
    queue_url: String,
}

pub(crate) async fn get_queue_url<S: SqsStore>(
    request: &ResolvedRequest,
    store: &S,
) -> Result<ServiceResponse, SqsError> {
    let request_body = serde_json::from_str::<GetQueueUrlRequest>(
        String::from_utf8(request.request.body.clone())
            .map_err(|e| SqsError::BadRequest(e.to_string()))?
            .as_str(),
    )
    .map_err(|e| SqsError::BadRequest(e.to_string()))?;

    let account_id = request_body
        .queue_owner_aws_account_id
        .unwrap_or_else(|| request.auth_context.principal.account_id.clone());

    let queue = store
        .get_queue(&request_body.queue_name, &request.region, &account_id)
        .await
        .map_err(|e| SqsError::StoreError(e))?;

    match queue {
        Some(queue) => {
            let response = GetQueueUrlResponse {
                queue_url: format!(
                    "http://{}/{}/{}",
                    request.request.host,
                    account_id.clone(),
                    request_body.queue_name
                ),
            };
            Ok(ServiceResponse {
                status_code: 200,
                headers: vec![],
                body: serde_json::to_vec(&response).unwrap_or_default(),
            })
        }
        None => Err(SqsError::QueueNotFound(
            request_body.queue_name,
            request.region.clone(),
            account_id,
        )),
    }
}

#[cfg(test)]
mod tests {
    use std::{
        collections::HashMap,
        sync::{Mutex, MutexGuard},
    };

    use async_trait::async_trait;
    use chrono::{TimeZone, Utc};
    use hiraeth_auth::{AuthContext, ResolvedRequest};
    use hiraeth_http::IncomingRequest;
    use hiraeth_store::{
        StoreError,
        principal::Principal,
        sqs::{SqsMessage, SqsQueue, SqsStore},
    };

    use super::delete_queue;
    use crate::SqsError;

    #[derive(Default)]
    struct TestSqsStore {
        queues: Mutex<HashMap<(String, String), SqsQueue>>,
        deleted_queue_ids: Mutex<Vec<i64>>,
    }

    impl TestSqsStore {
        fn with_queue(queue: SqsQueue) -> Self {
            let mut queues = HashMap::new();
            queues.insert((queue.name.clone(), queue.region.clone()), queue);

            Self {
                queues: Mutex::new(queues),
                deleted_queue_ids: Mutex::new(Vec::new()),
            }
        }

        fn deleted_queue_ids(&self) -> MutexGuard<'_, Vec<i64>> {
            self.deleted_queue_ids.lock().expect("deleted queue ids mutex")
        }
    }

    #[async_trait]
    impl SqsStore for TestSqsStore {
        async fn create_queue(&self, _queue: SqsQueue) -> Result<(), StoreError> {
            unimplemented!()
        }

        async fn delete_queue(&self, queue_id: i64) -> Result<(), StoreError> {
            self.deleted_queue_ids
                .lock()
                .expect("deleted queue ids mutex")
                .push(queue_id);
            Ok(())
        }

        async fn get_queue(
            &self,
            queue_name: &str,
            region: &str,
            account_id: &str,
        ) -> Result<Option<SqsQueue>, StoreError> {
            Ok(self
                .queues
                .lock()
                .expect("queues mutex")
                .values()
                .find(|queue| {
                    queue.name == queue_name
                        && queue.region == region
                        && queue.account_id == account_id
                })
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

        async fn send_message(&self, _message: &SqsMessage) -> Result<(), StoreError> {
            unimplemented!()
        }

        async fn receive_messages(
            &self,
            _queue_id: i64,
            _max_number_of_messages: i64,
            _visibility_timeout_seconds: u32,
        ) -> Result<Vec<SqsMessage>, StoreError> {
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

    fn resolved_request(body: &str) -> ResolvedRequest {
        let mut headers = HashMap::new();
        headers.insert(
            "x-amz-target".to_string(),
            "AmazonSQS.DeleteQueue".to_string(),
        );

        ResolvedRequest {
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
                    created_at: Utc
                        .with_ymd_and_hms(2026, 4, 4, 12, 0, 0)
                        .unwrap()
                        .naive_utc(),
                },
            },
            date: Utc.with_ymd_and_hms(2026, 4, 4, 12, 0, 0).unwrap(),
        }
    }

    fn queue() -> SqsQueue {
        SqsQueue {
            id: 42,
            name: "orders".to_string(),
            region: "us-east-1".to_string(),
            account_id: "123456789012".to_string(),
            queue_type: "standard".to_string(),
            visibility_timeout_seconds: 30,
            delay_seconds: 0,
            message_retention_period_seconds: 345600,
            receive_message_wait_time_seconds: 0,
            created_at: Utc
                .with_ymd_and_hms(2026, 4, 4, 11, 0, 0)
                .unwrap()
                .naive_utc(),
        }
    }

    #[tokio::test]
    async fn delete_queue_deletes_existing_queue() {
        let store = TestSqsStore::with_queue(queue());
        let request =
            resolved_request(r#"{"QueueUrl":"http://localhost:4566/123456789012/orders"}"#);

        let response = delete_queue(&request, &store)
            .await
            .expect("delete queue should succeed");

        assert_eq!(response.status_code, 200);
        assert!(response.body.is_empty());

        let deleted = store.deleted_queue_ids();
        assert_eq!(&*deleted, &[42]);
    }

    #[tokio::test]
    async fn delete_queue_returns_not_found_for_missing_queue() {
        let store = TestSqsStore::default();
        let request =
            resolved_request(r#"{"QueueUrl":"http://localhost:4566/123456789012/orders"}"#);

        let result = delete_queue(&request, &store).await;

        assert!(matches!(
            result,
            Err(SqsError::QueueNotFound(name, region, account))
                if name == "orders" && region == "us-east-1" && account == "123456789012"
        ));
    }
}
