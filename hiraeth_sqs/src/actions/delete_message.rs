use async_trait::async_trait;
use hiraeth_core::{
    ApiError, AwsActionPayloadFormat, AwsActionPayloadParseError, ResolvedRequest, ServiceResponse,
    TypedAwsAction, auth::AuthorizationCheck, empty_response,
};
use hiraeth_store::sqs::{SqsQueue, SqsStore};
use serde::Deserialize;

use super::action_support::{json_payload_format, parse_payload_error};
use crate::error::SqsError;

pub(crate) struct DeleteMessageAction;

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub(crate) struct DeleteMessageRequest {
    pub queue_url: String,
    pub receipt_handle: String,
}

async fn handle_delete_message_typed<S: SqsStore>(
    request: &ResolvedRequest,
    store: &S,
    delete_request: DeleteMessageRequest,
) -> Result<ServiceResponse, SqsError> {
    let queue = crate::util::load_queue_from_url(request, store, &delete_request.queue_url).await?;

    store
        .delete_message(queue.id, &delete_request.receipt_handle)
        .await
        .map_err(crate::error::map_receipt_handle_store_error)?;

    Ok(empty_response())
}

#[async_trait]
impl<S> TypedAwsAction<S> for DeleteMessageAction
where
    S: SqsStore + Send + Sync,
{
    type Request = DeleteMessageRequest;

    fn name(&self) -> &'static str {
        "DeleteMessage"
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
        delete_request: DeleteMessageRequest,
        store: &S,
    ) -> Result<ServiceResponse, ApiError> {
        match handle_delete_message_typed(&request, store, delete_request).await {
            Ok(response) => Ok(response),
            Err(error) => Ok(ServiceResponse::from(error)),
        }
    }

    async fn resolve_authorization_typed(
        &self,
        request: &ResolvedRequest,
        _payload: DeleteMessageRequest,
        store: &S,
    ) -> Result<AuthorizationCheck, ServiceResponse> {
        crate::auth::resolve_authorization("sqs:DeleteMessage", request, store).await
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use chrono::{TimeZone, Utc};
    use hiraeth_core::{AuthContext, ResolvedRequest, TypedAwsAction};
    use hiraeth_http::IncomingRequest;
    use hiraeth_store::{principal::Principal, sqs::SqsQueue, test_support::SqsTestStore};

    use super::{DeleteMessageAction, handle_delete_message_typed};
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
            "AmazonSQS.DeleteMessage".to_string(),
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

    #[test]
    fn reports_expected_action_name() {
        assert_eq!(
            <DeleteMessageAction as TypedAwsAction<SqsTestStore>>::name(&DeleteMessageAction),
            "DeleteMessage"
        );
    }

    #[tokio::test]
    async fn deletes_matching_receipt_handle() {
        let store = SqsTestStore::with_queue(queue());
        let request = resolved_request(
            r#"{
                "QueueUrl":"http://localhost:4566/123456789012/orders",
                "ReceiptHandle":"receipt-123"
            }"#,
        );

        let response = handle_delete_message_typed(
            &request,
            &store,
            crate::actions::test_support::parse_request_body(&request),
        )
        .await
        .expect("delete message should succeed");

        assert_eq!(response.status_code, 200);
        assert!(response.body.is_empty());
        assert_eq!(
            store.deleted_messages(),
            vec![(42, "receipt-123".to_string())]
        );
    }

    #[tokio::test]
    async fn returns_not_found_for_missing_queue() {
        let store = SqsTestStore::default();
        let request = resolved_request(
            r#"{
                "QueueUrl":"http://localhost:4566/123456789012/orders",
                "ReceiptHandle":"receipt-123"
            }"#,
        );

        let error = handle_delete_message_typed(
            &request,
            &store,
            crate::actions::test_support::parse_request_body(&request),
        )
        .await
        .expect_err("missing queue should error");

        assert_eq!(error, SqsError::QueueNotFound);
    }
}
