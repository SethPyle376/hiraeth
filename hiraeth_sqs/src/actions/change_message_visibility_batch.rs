use async_trait::async_trait;
use chrono::{Duration, Utc};
use hiraeth_core::{
    ApiError, AwsActionPayloadFormat, AwsActionPayloadParseError, ResolvedRequest, ServiceResponse,
    TypedAwsAction, auth::AuthorizationCheck, json_response,
};
use hiraeth_store::sqs::{SqsQueue, SqsStore};
use serde::{Deserialize, Serialize};

use super::{
    action_support::{json_payload_format, parse_payload_error},
    change_message_visibility::validate_visibility_timeout,
};
use crate::error::{SqsError, batch_error_details, map_receipt_handle_store_error};

pub(crate) struct ChangeMessageVisibilityBatchAction;

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub(crate) struct ChangeMessageVisibilityBatchEntry {
    pub id: String,
    pub receipt_handle: String,
    pub visibility_timeout: u32,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub(crate) struct ChangeMessageVisibilityBatchRequest {
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

async fn handle_change_message_visibility_batch_typed<S: SqsStore>(
    request: &ResolvedRequest,
    store: &S,
    change_request: ChangeMessageVisibilityBatchRequest,
) -> Result<ServiceResponse, SqsError> {
    let queue = crate::util::load_queue_from_url(request, store, &change_request.queue_url).await?;
    crate::util::validate_batch_request(
        change_request.entries.iter().map(|entry| entry.id.as_str()),
    )?;

    let mut successful = Vec::new();
    let mut failed = Vec::new();

    for entry in change_request.entries {
        let ChangeMessageVisibilityBatchEntry {
            id,
            receipt_handle,
            visibility_timeout,
        } = entry;
        if let Err(error) = validate_visibility_timeout(visibility_timeout) {
            failed.push(ChangeMessageVisibilityBatchFailedEntry {
                id,
                code: "InvalidParameterValue".to_string(),
                message: error.to_string(),
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
            Err(error) => {
                let error = map_receipt_handle_store_error(error);
                let (code, sender_fault) = batch_error_details(&error);
                failed.push(ChangeMessageVisibilityBatchFailedEntry {
                    id,
                    code: code.to_string(),
                    message: error.to_string(),
                    sender_fault,
                });
            }
        }
    }

    json_response(&ChangeMessageVisibilityBatchResponse { successful, failed }).map_err(Into::into)
}

#[async_trait]
impl<S> TypedAwsAction<S> for ChangeMessageVisibilityBatchAction
where
    S: SqsStore + Send + Sync,
{
    type Request = ChangeMessageVisibilityBatchRequest;

    fn name(&self) -> &'static str {
        "ChangeMessageVisibilityBatch"
    }

    fn payload_format(&self) -> AwsActionPayloadFormat {
        json_payload_format()
    }

    fn parse_error(&self, error: AwsActionPayloadParseError) -> ServiceResponse {
        parse_payload_error(error)
    }

    async fn handle_typed(
        &self,
        request: ResolvedRequest,
        change_request: ChangeMessageVisibilityBatchRequest,
        store: &S,
    ) -> Result<ServiceResponse, ApiError> {
        match handle_change_message_visibility_batch_typed(&request, store, change_request).await {
            Ok(response) => Ok(response),
            Err(error) => Ok(ServiceResponse::from(error)),
        }
    }

    async fn resolve_authorization_typed(
        &self,
        request: &ResolvedRequest,
        _payload: ChangeMessageVisibilityBatchRequest,
        store: &S,
    ) -> Result<AuthorizationCheck, ServiceResponse> {
        crate::auth::resolve_authorization("sqs:ChangeMessageVisibility", request, store).await
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use chrono::{TimeZone, Utc};
    use hiraeth_core::{AuthContext, ResolvedRequest, TypedAwsAction};
    use hiraeth_http::IncomingRequest;
    use hiraeth_router::ServiceResponse;
    use hiraeth_store::{principal::Principal, sqs::SqsQueue, test_support::SqsTestStore};
    use serde_json::Value;

    use super::{ChangeMessageVisibilityBatchAction, handle_change_message_visibility_batch_typed};

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
            "AmazonSQS.ChangeMessageVisibilityBatch".to_string(),
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
                    path: "/".to_string(),
                    user_id: "AIDATESTUSER000001".to_string(),
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

    #[test]
    fn reports_expected_action_name() {
        assert_eq!(
            <ChangeMessageVisibilityBatchAction as TypedAwsAction<SqsTestStore>>::name(
                &ChangeMessageVisibilityBatchAction
            ),
            "ChangeMessageVisibilityBatch"
        );
    }

    #[tokio::test]
    async fn returns_successful_and_failed_entries() {
        let store = SqsTestStore::with_queue(queue()).with_failing_receipt_handles(&["receipt-2"]);
        let request = resolved_request(
            r#"{
                "QueueUrl":"http://localhost:4566/123456789012/orders",
                "Entries":[
                    {"Id":"entry-1","ReceiptHandle":"receipt-1","VisibilityTimeout":45},
                    {"Id":"entry-2","ReceiptHandle":"receipt-2","VisibilityTimeout":45},
                    {"Id":"entry-3","ReceiptHandle":"receipt-3","VisibilityTimeout":50000}
                ]
            }"#,
        );

        let response = handle_change_message_visibility_batch_typed(
            &request,
            &store,
            crate::actions::test_support::parse_request_body(&request),
        )
        .await
        .expect("change visibility batch should succeed");

        assert_eq!(response.status_code, 200);
        let body = parse_json_body(&response);
        assert_eq!(body["Successful"].as_array().unwrap().len(), 1);
        assert_eq!(body["Failed"].as_array().unwrap().len(), 2);
        assert_eq!(body["Successful"][0]["Id"], "entry-1");
        assert_eq!(body["Failed"][0]["Id"], "entry-2");
        assert_eq!(body["Failed"][1]["Id"], "entry-3");
    }
}
