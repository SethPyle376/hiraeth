use chrono::{Duration, Utc};
use hiraeth_auth::ResolvedRequest;
use hiraeth_core::{ServiceResponse, empty_response, json_response};
use hiraeth_store::sqs::SqsStore;
use serde::{Deserialize, Serialize};

use crate::error::{SqsError, batch_error_details, map_receipt_handle_store_error};
use crate::util;

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct ChangeMessageVisibilityRequest {
    pub queue_url: String,
    pub receipt_handle: String,
    pub visibility_timeout: u32,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct ChangeMessageVisibilityBatchEntry {
    pub id: String,
    pub receipt_handle: String,
    pub visibility_timeout: u32,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct ChangeMessageVisibilityBatchRequest {
    pub queue_url: String,
    pub entries: Vec<ChangeMessageVisibilityBatchEntry>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "PascalCase")]
struct ChangeMessageVisibilityBatchSuccessEntry {
    pub id: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "PascalCase")]
struct ChangeMessageVisibilityBatchFailedEntry {
    pub id: String,
    pub code: String,
    pub message: String,
    pub sender_fault: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "PascalCase")]
struct ChangeMessageVisibilityBatchResponse {
    pub successful: Vec<ChangeMessageVisibilityBatchSuccessEntry>,
    pub failed: Vec<ChangeMessageVisibilityBatchFailedEntry>,
}

pub(crate) async fn change_message_visibility<S: SqsStore>(
    request: &ResolvedRequest,
    store: &S,
) -> Result<ServiceResponse, SqsError> {
    let change_request = util::parse_request_body::<ChangeMessageVisibilityRequest>(request)?;
    let queue = util::load_queue_from_url(request, store, &change_request.queue_url).await?;
    validate_visibility_timeout(change_request.visibility_timeout)?;

    store
        .set_message_visible_at(
            queue.id,
            &change_request.receipt_handle,
            (Utc::now() + Duration::seconds(change_request.visibility_timeout as i64)).naive_utc(),
        )
        .await
        .map_err(map_receipt_handle_store_error)?;

    Ok(empty_response())
}

pub(crate) async fn change_message_visibility_batch<S: SqsStore>(
    request: &ResolvedRequest,
    store: &S,
) -> Result<ServiceResponse, SqsError> {
    let change_request = util::parse_request_body::<ChangeMessageVisibilityBatchRequest>(request)?;
    let queue = util::load_queue_from_url(request, store, &change_request.queue_url).await?;
    util::validate_batch_request(change_request.entries.iter().map(|entry| entry.id.as_str()))?;

    let mut successful = Vec::new();
    let mut failed = Vec::new();

    for entry in change_request.entries {
        let ChangeMessageVisibilityBatchEntry {
            id,
            receipt_handle,
            visibility_timeout,
        } = entry;
        if let Err(e) = validate_visibility_timeout(visibility_timeout) {
            failed.push(ChangeMessageVisibilityBatchFailedEntry {
                id,
                code: "InvalidParameterValue".to_string(),
                message: e.to_string(),
                sender_fault: true,
            });
            continue;
        }

        let visible_at = (Utc::now() + Duration::seconds(visibility_timeout as i64)).naive_utc();

        match store
            .set_message_visible_at(queue.id, &receipt_handle, visible_at)
            .await
        {
            Ok(()) => successful.push(ChangeMessageVisibilityBatchSuccessEntry { id }),
            Err(e) => {
                let error = map_receipt_handle_store_error(e);
                let (code, sender_fault) = batch_error_details(&error);
                failed.push(ChangeMessageVisibilityBatchFailedEntry {
                    id,
                    code: code.to_string(),
                    message: error.to_string(),
                    sender_fault,
                })
            }
        }
    }

    json_response(&ChangeMessageVisibilityBatchResponse { successful, failed }).map_err(Into::into)
}

fn validate_visibility_timeout(visibility_timeout: u32) -> Result<(), SqsError> {
    if visibility_timeout > 43200 {
        return Err(SqsError::BadRequest(
            "VisibilityTimeout must be between 0 and 43200".to_string(),
        ));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use chrono::{Duration, TimeZone, Utc};
    use hiraeth_auth::{AuthContext, ResolvedRequest};
    use hiraeth_http::IncomingRequest;
    use hiraeth_router::ServiceResponse;
    use hiraeth_store::{principal::Principal, sqs::SqsQueue, test_support::SqsTestStore};
    use serde_json::Value;

    use super::{change_message_visibility, change_message_visibility_batch};
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

    fn resolved_request_for_target(target: &str, body: &str) -> ResolvedRequest {
        let mut request = resolved_request(body);
        request
            .request
            .headers
            .insert("x-amz-target".to_string(), target.to_string());
        request
    }

    fn parse_json_body(response: &ServiceResponse) -> Value {
        serde_json::from_slice(&response.body).expect("response body should be valid json")
    }

    #[tokio::test]
    async fn change_message_visibility_updates_visible_at_for_receipt_handle() {
        let store = SqsTestStore::with_queue(queue());
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
        let store = SqsTestStore::default();
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

    #[tokio::test]
    async fn change_message_visibility_batch_returns_successful_and_failed_entries() {
        let store = SqsTestStore::with_queue(queue()).with_failing_receipt_handles(&["receipt-2"]);
        let request = resolved_request_for_target(
            "AmazonSQS.ChangeMessageVisibilityBatch",
            r#"{
                "QueueUrl":"http://localhost:4566/123456789012/orders",
                "Entries":[
                    {"Id":"entry-1","ReceiptHandle":"receipt-1","VisibilityTimeout":45},
                    {"Id":"entry-2","ReceiptHandle":"receipt-2","VisibilityTimeout":90},
                    {"Id":"entry-3","ReceiptHandle":"receipt-3","VisibilityTimeout":0}
                ]
            }"#,
        );

        let before = Utc::now().naive_utc();
        let response = change_message_visibility_batch(&request, &store)
            .await
            .expect("change message visibility batch should succeed");
        let after = Utc::now().naive_utc();

        assert_eq!(response.status_code, 200);

        let body = parse_json_body(&response);
        assert_eq!(body["Successful"].as_array().unwrap().len(), 2);
        assert_eq!(body["Failed"].as_array().unwrap().len(), 1);
        assert_eq!(body["Successful"][0]["Id"], "entry-1");
        assert_eq!(body["Successful"][1]["Id"], "entry-3");
        assert_eq!(body["Failed"][0]["Id"], "entry-2");
        assert_eq!(body["Failed"][0]["Code"], "ReceiptHandleIsInvalid");
        assert_eq!(body["Failed"][0]["SenderFault"], true);

        let updates = store.visibility_updates();
        assert_eq!(updates.len(), 2);
        assert_eq!(updates[0].0, 42);
        assert_eq!(updates[0].1, "receipt-1");
        assert!(updates[0].2 >= before + Duration::seconds(45));
        assert!(updates[0].2 <= after + Duration::seconds(45));
        assert_eq!(updates[1].0, 42);
        assert_eq!(updates[1].1, "receipt-3");
        assert!(updates[1].2 >= before);
        assert!(updates[1].2 <= after);
    }

    #[tokio::test]
    async fn change_message_visibility_batch_returns_not_found_for_missing_queue() {
        let store = SqsTestStore::default();
        let request = resolved_request_for_target(
            "AmazonSQS.ChangeMessageVisibilityBatch",
            r#"{
                "QueueUrl":"http://localhost:4566/123456789012/orders",
                "Entries":[
                    {"Id":"entry-1","ReceiptHandle":"receipt-1","VisibilityTimeout":45}
                ]
            }"#,
        );

        let error = change_message_visibility_batch(&request, &store)
            .await
            .err()
            .expect("missing queue should error");

        assert_eq!(error, SqsError::QueueNotFound);
    }
}
