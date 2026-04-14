use hiraeth_auth::ResolvedRequest;
use hiraeth_router::ServiceResponse;
use hiraeth_store::sqs::SqsStore;
use serde::{Deserialize, Serialize};

use crate::{error::SqsError, util};

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct DeleteMessageRequest {
    pub queue_url: String,
    pub receipt_handle: String,
}

pub(crate) async fn delete_message<S: SqsStore>(
    request: &ResolvedRequest,
    store: &S,
) -> Result<ServiceResponse, SqsError> {
    let delete_request = util::parse_request_body::<DeleteMessageRequest>(request)?;
    let queue = util::load_queue_from_url(request, store, &delete_request.queue_url).await?;

    store
        .delete_message(queue.id, &delete_request.receipt_handle)
        .await
        .map_err(|e| SqsError::InternalError(e.to_string()))?;

    Ok(ServiceResponse {
        status_code: 200,
        headers: vec![],
        body: vec![],
    })
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct DeleteMessageBatchEntry {
    pub id: String,
    pub receipt_handle: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct DeleteMessageBatchRequest {
    pub queue_url: String,
    pub entries: Vec<DeleteMessageBatchEntry>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "PascalCase")]
struct DeleteMessageBatchSuccessEntry {
    pub id: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "PascalCase")]
struct DeleteMessageBatchFailedEntry {
    pub id: String,
    pub code: String,
    pub message: String,
    pub sender_fault: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "PascalCase")]
struct DeleteMessageBatchResponse {
    pub successful: Vec<DeleteMessageBatchSuccessEntry>,
    pub failed: Vec<DeleteMessageBatchFailedEntry>,
}

pub(crate) async fn delete_message_batch<S: SqsStore>(
    request: &ResolvedRequest,
    store: &S,
) -> Result<ServiceResponse, SqsError> {
    let delete_request = util::parse_request_body::<DeleteMessageBatchRequest>(request)?;
    let queue = util::load_queue_from_url(request, store, &delete_request.queue_url).await?;

    let mut successful = Vec::new();
    let mut failed = Vec::new();

    for entry in delete_request.entries {
        store
            .delete_message(queue.id, &entry.receipt_handle)
            .await
            .inspect_err(|e| {
                failed.push(DeleteMessageBatchFailedEntry {
                    id: entry.id.clone(),
                    code: "StoreError".to_string(),
                    message: format!("Failed to delete message: {:?}", e),
                    sender_fault: false,
                })
            })
            .inspect(|_| {
                successful.push(DeleteMessageBatchSuccessEntry {
                    id: entry.id.clone(),
                })
            });
    }

    let response = DeleteMessageBatchResponse { successful, failed };

    Ok(ServiceResponse {
        status_code: 200,
        headers: vec![],
        body: serde_json::to_vec(&response).map_err(|e| SqsError::BadRequest(e.to_string()))?,
    })
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use chrono::{TimeZone, Utc};
    use hiraeth_auth::{AuthContext, ResolvedRequest};
    use hiraeth_http::IncomingRequest;
    use hiraeth_router::ServiceResponse;
    use hiraeth_store::{principal::Principal, sqs::SqsQueue, test_support::SqsTestStore};
    use serde_json::Value;

    use super::{delete_message, delete_message_batch};
    use crate::error::SqsError;

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
            updated_at: Utc
                .with_ymd_and_hms(2026, 4, 5, 11, 0, 0)
                .unwrap()
                .naive_utc(),
            ..Default::default()
        }
    }

    fn resolved_request(target: &str, body: &str) -> ResolvedRequest {
        let mut headers = HashMap::new();
        headers.insert("x-amz-target".to_string(), target.to_string());

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

    fn parse_json_body(response: &ServiceResponse) -> Value {
        serde_json::from_slice(&response.body).expect("response body should be valid json")
    }

    #[tokio::test]
    async fn delete_message_deletes_matching_receipt_handle() {
        let store = SqsTestStore::with_queue(queue());
        let request = resolved_request(
            "AmazonSQS.DeleteMessage",
            r#"{
                "QueueUrl":"http://localhost:4566/123456789012/orders",
                "ReceiptHandle":"receipt-123"
            }"#,
        );

        let response = delete_message(&request, &store)
            .await
            .expect("delete message should succeed");

        assert_eq!(response.status_code, 200);
        assert!(response.body.is_empty());

        let deleted = store.deleted_messages();
        assert_eq!(deleted, vec![(42, "receipt-123".to_string())]);
    }

    #[tokio::test]
    async fn delete_message_returns_not_found_for_missing_queue() {
        let store = SqsTestStore::default();
        let request = resolved_request(
            "AmazonSQS.DeleteMessage",
            r#"{
                "QueueUrl":"http://localhost:4566/123456789012/orders",
                "ReceiptHandle":"receipt-123"
            }"#,
        );

        let error = delete_message(&request, &store)
            .await
            .err()
            .expect("missing queue should error");

        assert_eq!(error, SqsError::QueueNotFound);
    }

    #[tokio::test]
    async fn delete_message_batch_returns_successful_and_failed_entries() {
        let store = SqsTestStore::with_queue(queue()).with_failing_receipt_handles(&["receipt-2"]);
        let request = resolved_request(
            "AmazonSQS.DeleteMessageBatch",
            r#"{
                "QueueUrl":"http://localhost:4566/123456789012/orders",
                "Entries":[
                    {"Id":"entry-1","ReceiptHandle":"receipt-1"},
                    {"Id":"entry-2","ReceiptHandle":"receipt-2"},
                    {"Id":"entry-3","ReceiptHandle":"receipt-3"}
                ]
            }"#,
        );

        let response = delete_message_batch(&request, &store)
            .await
            .expect("delete message batch should succeed");

        assert_eq!(response.status_code, 200);

        let body = parse_json_body(&response);
        assert_eq!(body["Successful"].as_array().unwrap().len(), 2);
        assert_eq!(body["Failed"].as_array().unwrap().len(), 1);
        assert_eq!(body["Successful"][0]["Id"], "entry-1");
        assert_eq!(body["Successful"][1]["Id"], "entry-3");
        assert_eq!(body["Failed"][0]["Id"], "entry-2");
        assert_eq!(body["Failed"][0]["Code"], "StoreError");
        assert_eq!(body["Failed"][0]["SenderFault"], false);

        let deleted = store.deleted_messages();
        assert_eq!(
            deleted,
            vec![(42, "receipt-1".to_string()), (42, "receipt-3".to_string())]
        );
    }
}
