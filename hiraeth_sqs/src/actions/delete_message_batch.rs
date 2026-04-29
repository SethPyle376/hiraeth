use std::collections::HashMap;

use async_trait::async_trait;
use hiraeth_core::{
    AwsActionPayloadFormat, AwsActionPayloadParseError, ResolvedRequest, ServiceResponse,
    TypedAwsAction, auth::AuthorizationCheck, json_response,
};
use hiraeth_store::sqs::{SqsQueue, SqsStore};
use serde::{Deserialize, Serialize};

use super::action_support::{json_payload_format, parse_payload_error};
use crate::error::{SqsError, batch_error_details, map_receipt_handle_store_error};

pub(crate) struct DeleteMessageBatchAction;

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub(crate) struct DeleteMessageBatchEntry {
    pub id: String,
    pub receipt_handle: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub(crate) struct DeleteMessageBatchRequest {
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

async fn handle_delete_message_batch_typed<S: SqsStore>(
    request: &ResolvedRequest,
    store: &S,
    delete_request: DeleteMessageBatchRequest,
) -> Result<ServiceResponse, SqsError> {
    let queue = crate::util::load_queue_from_url(request, store, &delete_request.queue_url).await?;
    crate::util::validate_batch_request(
        delete_request.entries.iter().map(|entry| entry.id.as_str()),
    )?;

    let mut successful = Vec::new();
    let mut failed = Vec::new();

    for entry in delete_request.entries {
        let result = store.delete_message(queue.id, &entry.receipt_handle).await;

        match result {
            Ok(()) => successful.push(DeleteMessageBatchSuccessEntry {
                id: entry.id.clone(),
            }),
            Err(e) => {
                let error = map_receipt_handle_store_error(e.clone());
                let (code, sender_fault) = batch_error_details(&error);
                failed.push(DeleteMessageBatchFailedEntry {
                    id: entry.id.clone(),
                    code: code.to_string(),
                    message: error.to_string(),
                    sender_fault,
                });
            }
        }
    }

    json_response(&DeleteMessageBatchResponse { successful, failed }).map_err(Into::into)
}

#[async_trait]
impl<S> TypedAwsAction<S> for DeleteMessageBatchAction
where
    S: SqsStore + Send + Sync,
{
    type Request = DeleteMessageBatchRequest;
    type Error = SqsError;

    fn name(&self) -> &'static str {
        "DeleteMessageBatch"
    }

    fn payload_format(&self) -> AwsActionPayloadFormat {
        json_payload_format()
    }

    fn parse_error(&self, error: AwsActionPayloadParseError) -> SqsError {
        parse_payload_error(error)
    }

    async fn handle(
        &self,
        request: ResolvedRequest,
        delete_request: DeleteMessageBatchRequest,
        store: &S,
        trace_context: &hiraeth_core::tracing::TraceContext,
        trace_recorder: &dyn hiraeth_core::tracing::TraceRecorder,
    ) -> Result<ServiceResponse, SqsError> {
        let timer = trace_context.start_span();
        let attributes = HashMap::from([
            ("queue_url".to_string(), delete_request.queue_url.clone()),
            (
                "entry_count".to_string(),
                delete_request.entries.len().to_string(),
            ),
        ]);

        let result = handle_delete_message_batch_typed(&request, store, delete_request).await;
        let status = if result.is_ok() { "ok" } else { "error" };
        trace_context
            .record_span_or_warn(
                trace_recorder,
                timer,
                "sqs.delete_message_batch.delete",
                "sqs",
                status,
                attributes,
            )
            .await;

        result
    }

    async fn resolve_authorization_typed(
        &self,
        request: &ResolvedRequest,
        _payload: DeleteMessageBatchRequest,
        store: &S,
    ) -> Result<AuthorizationCheck, SqsError> {
        crate::auth::resolve_authorization("sqs:DeleteMessage", request, store).await
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

    use super::{DeleteMessageBatchAction, handle_delete_message_batch_typed};

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
            "AmazonSQS.DeleteMessageBatch".to_string(),
        );

        ResolvedRequest {
            request_id: "test-request-id".to_string(),
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
            <DeleteMessageBatchAction as TypedAwsAction<SqsTestStore>>::name(
                &DeleteMessageBatchAction
            ),
            "DeleteMessageBatch"
        );
    }

    #[tokio::test]
    async fn returns_successful_and_failed_entries() {
        let store = SqsTestStore::with_queue(queue()).with_failing_receipt_handles(&["receipt-2"]);
        let request = resolved_request(
            r#"{
                "QueueUrl":"http://localhost:4566/123456789012/orders",
                "Entries":[
                    {"Id":"entry-1","ReceiptHandle":"receipt-1"},
                    {"Id":"entry-2","ReceiptHandle":"receipt-2"},
                    {"Id":"entry-3","ReceiptHandle":"receipt-3"}
                ]
            }"#,
        );

        let response = handle_delete_message_batch_typed(
            &request,
            &store,
            crate::actions::test_support::parse_request_body(&request),
        )
        .await
        .expect("delete message batch should succeed");

        assert_eq!(response.status_code, 200);

        let body = parse_json_body(&response);
        assert_eq!(body["Successful"].as_array().unwrap().len(), 2);
        assert_eq!(body["Failed"].as_array().unwrap().len(), 1);
        assert_eq!(body["Successful"][0]["Id"], "entry-1");
        assert_eq!(body["Successful"][1]["Id"], "entry-3");
        assert_eq!(body["Failed"][0]["Id"], "entry-2");
        assert_eq!(body["Failed"][0]["Code"], "ReceiptHandleIsInvalid");
        assert_eq!(body["Failed"][0]["SenderFault"], true);
        assert_eq!(
            store.deleted_messages(),
            vec![(42, "receipt-1".to_string()), (42, "receipt-3".to_string())]
        );
    }
}
