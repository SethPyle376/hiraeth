use std::collections::HashMap;

use hiraeth_auth::ResolvedRequest;
use hiraeth_router::ServiceResponse;
use hiraeth_store::sqs::{SqsQueue, SqsStore};
use serde::{Deserialize, Serialize};

use crate::{error::SqsError, util};

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
        .map_err(|e| SqsError::InternalError(e.to_string()))
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
        .map_err(|e| SqsError::InternalError(e.to_string()))?
        .ok_or_else(|| SqsError::QueueNotFound)?;

    store
        .delete_queue(queue.id)
        .await
        .map(|_| ServiceResponse {
            status_code: 200,
            headers: vec![],
            body: vec![],
        })
        .map_err(|e| SqsError::InternalError(e.to_string()))
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
        .map_err(|e| SqsError::InternalError(e.to_string()))?;

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
        None => Err(SqsError::QueueNotFound),
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct PurgeQueueRequest {
    queue_url: String,
}

pub(crate) async fn purge_queue<S: SqsStore>(
    request: &ResolvedRequest,
    store: &S,
) -> Result<ServiceResponse, SqsError> {
    let request_body = serde_json::from_str::<PurgeQueueRequest>(
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
        .map_err(|e| SqsError::InternalError(e.to_string()))?
        .ok_or_else(|| SqsError::QueueNotFound)?;

    store
        .purge_queue(queue.id)
        .await
        .map(|_| ServiceResponse {
            status_code: 200,
            headers: vec![],
            body: vec![],
        })
        .map_err(|e| SqsError::InternalError(e.to_string()))
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use chrono::{TimeZone, Utc};
    use hiraeth_auth::{AuthContext, ResolvedRequest};
    use hiraeth_http::IncomingRequest;
    use hiraeth_store::{principal::Principal, sqs::SqsQueue, test_support::SqsTestStore};

    use super::{delete_queue, purge_queue};
    use crate::error::SqsError;

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
        let store = SqsTestStore::with_queue(queue());
        let request = resolved_request(
            "AmazonSQS.DeleteQueue",
            r#"{"QueueUrl":"http://localhost:4566/123456789012/orders"}"#,
        );

        let response = delete_queue(&request, &store)
            .await
            .expect("delete queue should succeed");

        assert_eq!(response.status_code, 200);
        assert!(response.body.is_empty());

        let deleted = store.deleted_queue_ids();
        assert_eq!(deleted, vec![42]);
    }

    #[tokio::test]
    async fn delete_queue_returns_not_found_for_missing_queue() {
        let store = SqsTestStore::default();
        let request = resolved_request(
            "AmazonSQS.DeleteQueue",
            r#"{"QueueUrl":"http://localhost:4566/123456789012/orders"}"#,
        );

        let result = delete_queue(&request, &store).await;

        assert!(matches!(result, Err(SqsError::QueueNotFound)));
    }

    #[tokio::test]
    async fn purge_queue_purges_existing_queue() {
        let store = SqsTestStore::with_queue(queue());
        let request = resolved_request(
            "AmazonSQS.PurgeQueue",
            r#"{"QueueUrl":"http://localhost:4566/123456789012/orders"}"#,
        );

        let response = purge_queue(&request, &store)
            .await
            .expect("purge queue should succeed");

        assert_eq!(response.status_code, 200);
        assert!(response.body.is_empty());
        assert_eq!(store.purged_queue_ids(), vec![42]);
    }

    #[tokio::test]
    async fn purge_queue_returns_not_found_for_missing_queue() {
        let store = SqsTestStore::default();
        let request = resolved_request(
            "AmazonSQS.PurgeQueue",
            r#"{"QueueUrl":"http://localhost:4566/123456789012/orders"}"#,
        );

        let result = purge_queue(&request, &store).await;

        assert!(matches!(result, Err(SqsError::QueueNotFound)));
        assert!(store.purged_queue_ids().is_empty());
    }
}
