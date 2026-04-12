use chrono::{Duration, Utc};
use hiraeth_auth::ResolvedRequest;
use hiraeth_router::ServiceResponse;
use hiraeth_store::sqs::SqsStore;
use serde::Deserialize;

use crate::error::SqsError;
use crate::util;

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct ChangeMessageVisibilityRequest {
    pub queue_url: String,
    pub receipt_handle: String,
    pub visibility_timeout: u32,
}

pub(crate) async fn change_message_visibility<S: SqsStore>(
    request: &ResolvedRequest,
    store: &S,
) -> Result<ServiceResponse, SqsError> {
    let change_request = serde_json::from_str::<ChangeMessageVisibilityRequest>(
        String::from_utf8(request.request.body.clone())
            .map_err(|e| SqsError::BadRequest(e.to_string()))?
            .as_str(),
    )
    .map_err(|e| SqsError::BadRequest(e.to_string()))?;

    let queue_id = util::parse_queue_url(&change_request.queue_url, &request.region)
        .ok_or_else(|| SqsError::BadRequest("Invalid queue url".to_string()))?;

    let queue = store
        .get_queue(&queue_id.name, &queue_id.region, &queue_id.account_id)
        .await
        .map_err(|e| SqsError::InternalError(e.to_string()))?
        .ok_or_else(|| SqsError::QueueNotFound)?;

    store
        .set_message_visible_at(
            queue.id,
            &change_request.receipt_handle,
            (Utc::now() + Duration::seconds(change_request.visibility_timeout as i64)).naive_utc(),
        )
        .await
        .map_err(|e| SqsError::InternalError(e.to_string()))?;

    Ok(ServiceResponse {
        status_code: 200,
        headers: vec![],
        body: vec![],
    })
}

#[cfg(test)]
mod tests {
    use std::{
        collections::HashMap,
        sync::{Mutex, MutexGuard},
    };

    use async_trait::async_trait;
    use chrono::{Duration, TimeZone, Utc};
    use hiraeth_auth::{AuthContext, ResolvedRequest};
    use hiraeth_http::IncomingRequest;
    use hiraeth_store::{
        StoreError,
        principal::Principal,
        sqs::{SqsMessage, SqsQueue, SqsStore},
    };

    use super::change_message_visibility;
    use crate::error::SqsError;

    struct TestSqsStore {
        queue: Option<SqsQueue>,
        visibility_updates: Mutex<Vec<(i64, String, chrono::NaiveDateTime)>>,
    }

    impl TestSqsStore {
        fn with_queue(queue: SqsQueue) -> Self {
            Self {
                queue: Some(queue),
                visibility_updates: Mutex::new(Vec::new()),
            }
        }

        fn visibility_updates(&self) -> MutexGuard<'_, Vec<(i64, String, chrono::NaiveDateTime)>> {
            self.visibility_updates
                .lock()
                .expect("visibility updates mutex")
        }
    }

    #[async_trait]
    impl SqsStore for TestSqsStore {
        async fn create_queue(&self, _queue: SqsQueue) -> Result<(), StoreError> {
            unimplemented!()
        }

        async fn delete_queue(&self, _queue_id: i64) -> Result<(), StoreError> {
            unimplemented!()
        }

        async fn get_queue(
            &self,
            queue_name: &str,
            region: &str,
            account_id: &str,
        ) -> Result<Option<SqsQueue>, StoreError> {
            Ok(self
                .queue
                .as_ref()
                .filter(|queue| {
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
            queue_id: i64,
            receipt_handle: &str,
            visible_at: chrono::NaiveDateTime,
        ) -> Result<(), StoreError> {
            self.visibility_updates
                .lock()
                .expect("visibility updates mutex")
                .push((queue_id, receipt_handle.to_string(), visible_at));
            Ok(())
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
                .with_ymd_and_hms(2026, 4, 5, 11, 0, 0)
                .unwrap()
                .naive_utc(),
        }
    }

    fn resolved_request(body: &str) -> ResolvedRequest {
        let mut headers = HashMap::new();
        headers.insert(
            "x-amz-target".to_string(),
            "AmazonSQS.ChangeMessageVisibility".to_string(),
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
                        .with_ymd_and_hms(2026, 4, 5, 12, 0, 0)
                        .unwrap()
                        .naive_utc(),
                },
            },
            date: Utc.with_ymd_and_hms(2026, 4, 5, 12, 0, 0).unwrap(),
        }
    }

    #[tokio::test]
    async fn change_message_visibility_updates_visible_at_for_receipt_handle() {
        let store = TestSqsStore::with_queue(queue());
        let request = resolved_request(
            r#"{
                "QueueUrl":"http://localhost:4566/123456789012/orders",
                "ReceiptHandle":"receipt-123",
                "VisibilityTimeout":45
            }"#,
        );

        let before = Utc::now().naive_utc();
        let response = change_message_visibility(&request, &store)
            .await
            .expect("change message visibility should succeed");
        let after = Utc::now().naive_utc();

        assert_eq!(response.status_code, 200);
        assert!(response.body.is_empty());

        let updates = store.visibility_updates();
        assert_eq!(updates.len(), 1);
        assert_eq!(updates[0].0, 42);
        assert_eq!(updates[0].1, "receipt-123");
        assert!(updates[0].2 >= before + Duration::seconds(45));
        assert!(updates[0].2 <= after + Duration::seconds(45));
    }

    #[tokio::test]
    async fn change_message_visibility_returns_not_found_for_missing_queue() {
        let store = TestSqsStore {
            queue: None,
            visibility_updates: Mutex::new(Vec::new()),
        };
        let request = resolved_request(
            r#"{
                "QueueUrl":"http://localhost:4566/123456789012/orders",
                "ReceiptHandle":"receipt-123",
                "VisibilityTimeout":45
            }"#,
        );

        let error = change_message_visibility(&request, &store)
            .await
            .err()
            .expect("missing queue should error");

        assert_eq!(error, SqsError::QueueNotFound);
    }
}
