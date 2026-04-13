use hiraeth_auth::ResolvedRequest;
use hiraeth_router::ServiceResponse;
use hiraeth_store::sqs::SqsStore;
use serde::{Deserialize, Serialize};

use crate::error::SqsError;

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct ListQueuesRequest {
    max_results: Option<i64>,
    next_token: Option<String>,
    queue_name_prefix: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "PascalCase")]
struct ListQueuesResponse {
    queue_urls: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    next_token: Option<String>,
}

pub(crate) async fn list_queues<S: SqsStore>(
    request: &ResolvedRequest,
    store: &S,
) -> Result<ServiceResponse, SqsError> {
    let request_body = serde_json::from_str::<ListQueuesRequest>(
        String::from_utf8(request.request.body.clone())
            .map_err(|e| SqsError::BadRequest(e.to_string()))?
            .as_str(),
    )
    .map_err(|e| SqsError::BadRequest(e.to_string()))?;

    if let Some(max_results) = request_body.max_results {
        if !(1..=1000).contains(&max_results) {
            return Err(SqsError::BadRequest(
                "MaxResults must be between 1 and 1000".to_string(),
            ));
        }
    }

    let region = &request.region;
    let account_id = request.auth_context.principal.account_id.clone();

    let queue_name_prefix = request_body.queue_name_prefix.as_deref();
    let next_token = request_body.next_token.as_deref();

    let store_max_results = request_body
        .max_results
        .map(|max_results| max_results.saturating_add(1));

    let mut queues = store
        .list_queues(
            region,
            &account_id,
            queue_name_prefix,
            store_max_results,
            next_token,
        )
        .await
        .map_err(|e| SqsError::InternalError(e.to_string()))?;

    let next_token = if let Some(max_results) = request_body.max_results {
        if queues.len() as i64 > max_results {
            queues.truncate(max_results as usize);
            queues.last().map(|q| q.name.clone())
        } else {
            None
        }
    } else {
        None
    };

    let queue_urls = queues
        .into_iter()
        .map(|q| format!("http://{}/{}/{}", request.request.host, account_id, q.name))
        .collect();

    let list_response = ListQueuesResponse {
        queue_urls,
        next_token,
    };

    Ok(ServiceResponse {
        status_code: 200,
        headers: vec![],
        body: serde_json::to_vec(&list_response).unwrap_or_default(),
    })
}

#[cfg(test)]
mod tests {
    use std::sync::{Mutex, MutexGuard};

    use async_trait::async_trait;
    use chrono::{TimeZone, Utc};
    use hiraeth_auth::{AuthContext, ResolvedRequest};
    use hiraeth_http::IncomingRequest;
    use hiraeth_store::{
        StoreError,
        principal::Principal,
        sqs::{SqsMessage, SqsQueue, SqsStore},
    };
    use serde_json::Value;

    use super::{SqsError, list_queues};

    #[derive(Debug, Clone, PartialEq, Eq)]
    struct ListQueuesCall {
        region: String,
        account_id: String,
        queue_name_prefix: Option<String>,
        max_results: Option<i64>,
        next_token: Option<String>,
    }

    struct TestSqsStore {
        queues: Vec<SqsQueue>,
        calls: Mutex<Vec<ListQueuesCall>>,
    }

    impl TestSqsStore {
        fn new(queues: Vec<SqsQueue>) -> Self {
            Self {
                queues,
                calls: Mutex::new(Vec::new()),
            }
        }

        fn calls(&self) -> MutexGuard<'_, Vec<ListQueuesCall>> {
            self.calls.lock().expect("list queues calls mutex")
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
            _queue_name: &str,
            _region: &str,
            _account_id: &str,
        ) -> Result<Option<SqsQueue>, StoreError> {
            unimplemented!()
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
            region: &str,
            account_id: &str,
            queue_name_prefix: Option<&str>,
            max_results: Option<i64>,
            next_token: Option<&str>,
        ) -> Result<Vec<SqsQueue>, StoreError> {
            self.calls
                .lock()
                .expect("list queues calls mutex")
                .push(ListQueuesCall {
                    region: region.to_string(),
                    account_id: account_id.to_string(),
                    queue_name_prefix: queue_name_prefix.map(str::to_string),
                    max_results,
                    next_token: next_token.map(str::to_string),
                });

            let mut queues = self
                .queues
                .iter()
                .filter(|queue| queue.region == region && queue.account_id == account_id)
                .filter(|queue| {
                    queue_name_prefix
                        .map(|prefix| queue.name.starts_with(prefix))
                        .unwrap_or(true)
                })
                .filter(|queue| {
                    next_token
                        .map(|token| queue.name.as_str() > token)
                        .unwrap_or(true)
                })
                .cloned()
                .collect::<Vec<_>>();

            queues.sort_by(|left, right| left.name.cmp(&right.name));

            if let Some(max_results) = max_results {
                queues.truncate(max_results as usize);
            }

            Ok(queues)
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
        ResolvedRequest {
            request: IncomingRequest {
                host: "localhost:4566".to_string(),
                method: "POST".to_string(),
                path: "/".to_string(),
                query: None,
                headers: [(
                    "x-amz-target".to_string(),
                    "AmazonSQS.ListQueues".to_string(),
                )]
                .into_iter()
                .collect(),
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
                        .with_ymd_and_hms(2026, 4, 6, 12, 0, 0)
                        .unwrap()
                        .naive_utc(),
                },
            },
            date: Utc.with_ymd_and_hms(2026, 4, 6, 12, 0, 0).unwrap(),
        }
    }

    fn queue(name: &str, region: &str, account_id: &str) -> SqsQueue {
        SqsQueue {
            id: 0,
            name: name.to_string(),
            region: region.to_string(),
            account_id: account_id.to_string(),
            queue_type: "standard".to_string(),
            visibility_timeout_seconds: 30,
            delay_seconds: 0,
            message_retention_period_seconds: 345600,
            receive_message_wait_time_seconds: 0,
            created_at: Utc
                .with_ymd_and_hms(2026, 4, 6, 12, 0, 0)
                .unwrap()
                .naive_utc(),
        }
    }

    fn parse_json_body(response: &hiraeth_router::ServiceResponse) -> Value {
        serde_json::from_slice(&response.body).expect("response body should be valid json")
    }

    #[tokio::test]
    async fn list_queues_returns_matching_queue_urls_and_forwards_filters() {
        let store = TestSqsStore::new(vec![
            queue("orders-001", "us-east-1", "123456789012"),
            queue("orders-002", "us-east-1", "123456789012"),
            queue("orders-003", "us-east-1", "123456789012"),
            queue("payments-001", "us-east-1", "123456789012"),
            queue("orders-west", "us-west-2", "123456789012"),
            queue("orders-other-account", "us-east-1", "999999999999"),
        ]);
        let request = resolved_request(
            r#"{
                "QueueNamePrefix":"orders-",
                "MaxResults":2,
                "NextToken":"orders-001"
            }"#,
        );

        let response = list_queues(&request, &store)
            .await
            .expect("list queues should succeed");
        let body = parse_json_body(&response);

        assert_eq!(response.status_code, 200);
        assert_eq!(
            body["QueueUrls"],
            serde_json::json!([
                "http://localhost:4566/123456789012/orders-002",
                "http://localhost:4566/123456789012/orders-003"
            ])
        );
        assert!(body.get("NextToken").is_none());

        let calls = store.calls();
        assert_eq!(calls.len(), 1);
        assert_eq!(
            calls[0],
            ListQueuesCall {
                region: "us-east-1".to_string(),
                account_id: "123456789012".to_string(),
                queue_name_prefix: Some("orders-".to_string()),
                max_results: Some(3),
                next_token: Some("orders-001".to_string()),
            }
        );
    }

    #[tokio::test]
    async fn list_queues_returns_next_token_when_another_page_exists() {
        let store = TestSqsStore::new(vec![
            queue("orders-001", "us-east-1", "123456789012"),
            queue("orders-002", "us-east-1", "123456789012"),
            queue("orders-003", "us-east-1", "123456789012"),
        ]);
        let request = resolved_request(r#"{"MaxResults":2}"#);

        let response = list_queues(&request, &store)
            .await
            .expect("list queues should succeed");
        let body = parse_json_body(&response);

        assert_eq!(
            body["QueueUrls"],
            serde_json::json!([
                "http://localhost:4566/123456789012/orders-001",
                "http://localhost:4566/123456789012/orders-002"
            ])
        );
        assert_eq!(body["NextToken"], "orders-002");
    }

    #[tokio::test]
    async fn list_queues_omits_next_token_when_page_is_exactly_full() {
        let store = TestSqsStore::new(vec![
            queue("orders-001", "us-east-1", "123456789012"),
            queue("orders-002", "us-east-1", "123456789012"),
        ]);
        let request = resolved_request(r#"{"MaxResults":2}"#);

        let response = list_queues(&request, &store)
            .await
            .expect("list queues should succeed");
        let body = parse_json_body(&response);

        assert_eq!(
            body["QueueUrls"],
            serde_json::json!([
                "http://localhost:4566/123456789012/orders-001",
                "http://localhost:4566/123456789012/orders-002"
            ])
        );
        assert!(body.get("NextToken").is_none());
    }

    #[tokio::test]
    async fn list_queues_rejects_invalid_max_results() {
        let store = TestSqsStore::new(Vec::new());
        let request = resolved_request(r#"{"MaxResults":0}"#);

        let result = list_queues(&request, &store).await;

        assert!(matches!(
            result,
            Err(SqsError::BadRequest(message))
                if message == "MaxResults must be between 1 and 1000"
        ));
        assert!(store.calls().is_empty());
    }
}
