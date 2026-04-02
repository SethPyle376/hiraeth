use std::collections::HashMap;

use hiraeth_auth::ResolvedRequest;
use hiraeth_router::ServiceResponse;
use hiraeth_store::sqs::{SqsQueue, SqsStore};
use serde::{Deserialize, Serialize};

use crate::SqsError;

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
                    request.auth_context.principal.account_id.clone(),
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
