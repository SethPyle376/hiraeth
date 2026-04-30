use std::collections::HashMap;

use async_trait::async_trait;
use hiraeth_core::{
    AwsActionPayloadFormat, AwsActionPayloadParseError, AwsActionResponseFormat, ResolvedRequest,
    TypedAwsAction,
    auth::AuthorizationCheck,
    tracing::{TraceContext, TraceRecorder},
};
use hiraeth_store::sqs::{SqsQueue, SqsStore};
use serde::Deserialize;

use super::action_support::{json_payload_format, parse_payload_error};
use crate::error::SqsError;

pub(crate) struct DeleteQueueAction;

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub(crate) struct DeleteQueueRequest {
    queue_url: String,
}

async fn handle_delete_queue_typed<S: SqsStore>(
    request: &ResolvedRequest,
    store: &S,
    request_body: DeleteQueueRequest,
) -> Result<(), SqsError> {
    let queue = crate::util::load_queue_from_url(request, store, &request_body.queue_url).await?;

    store
        .delete_queue(queue.id)
        .await
        .map_err(crate::error::map_store_error)
}

#[async_trait]
impl<S> TypedAwsAction<S> for DeleteQueueAction
where
    S: SqsStore + Send + Sync,
{
    type Request = DeleteQueueRequest;
    type Response = ();
    type Error = SqsError;

    fn name(&self) -> &'static str {
        "DeleteQueue"
    }

    fn payload_format(&self) -> AwsActionPayloadFormat {
        json_payload_format()
    }

    fn response_format(&self) -> AwsActionResponseFormat {
        AwsActionResponseFormat::Empty
    }

    fn parse_error(&self, error: AwsActionPayloadParseError) -> SqsError {
        parse_payload_error(error)
    }

    async fn handle(
        &self,
        request: ResolvedRequest,
        request_body: DeleteQueueRequest,
        store: &S,
        trace_context: &TraceContext,
        trace_recorder: &dyn TraceRecorder,
    ) -> Result<(), SqsError> {
        let attributes = HashMap::from([("queue_url".to_string(), request_body.queue_url.clone())]);

        trace_context
            .record_result_span(
                trace_recorder,
                "sqs.queue.delete",
                "sqs",
                attributes,
                async { handle_delete_queue_typed(&request, store, request_body).await },
            )
            .await
    }

    async fn resolve_authorization_typed(
        &self,
        request: &ResolvedRequest,
        _payload: DeleteQueueRequest,
        store: &S,
    ) -> Result<AuthorizationCheck, SqsError> {
        crate::auth::resolve_authorization("sqs:DeleteQueue", request, store).await
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use chrono::{TimeZone, Utc};
    use hiraeth_core::{AuthContext, ResolvedRequest, TypedAwsAction};
    use hiraeth_http::IncomingRequest;
    use hiraeth_store::{principal::Principal, sqs::SqsQueue, test_support::SqsTestStore};

    use super::{DeleteQueueAction, handle_delete_queue_typed};
    use crate::error::SqsError;

    fn resolved_request(body: &str) -> ResolvedRequest {
        let mut headers = HashMap::new();
        headers.insert(
            "x-amz-target".to_string(),
            "AmazonSQS.DeleteQueue".to_string(),
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
            updated_at: Utc
                .with_ymd_and_hms(2026, 4, 4, 11, 0, 0)
                .unwrap()
                .naive_utc(),
            ..Default::default()
        }
    }

    #[test]
    fn reports_expected_action_name() {
        assert_eq!(
            <DeleteQueueAction as TypedAwsAction<SqsTestStore>>::name(&DeleteQueueAction),
            "DeleteQueue"
        );
    }

    #[tokio::test]
    async fn deletes_existing_queue() {
        let store = SqsTestStore::with_queue(queue());
        let request =
            resolved_request(r#"{"QueueUrl":"http://localhost:4566/123456789012/orders"}"#);

        handle_delete_queue_typed(
            &request,
            &store,
            crate::actions::test_support::parse_request_body(&request),
        )
        .await
        .expect("delete queue should succeed");
        assert_eq!(store.deleted_queue_ids(), vec![42]);
    }

    #[tokio::test]
    async fn returns_not_found_for_missing_queue() {
        let store = SqsTestStore::default();
        let request =
            resolved_request(r#"{"QueueUrl":"http://localhost:4566/123456789012/orders"}"#);

        let result = handle_delete_queue_typed(
            &request,
            &store,
            crate::actions::test_support::parse_request_body(&request),
        )
        .await;

        assert!(matches!(result, Err(SqsError::QueueNotFound)));
    }
}
