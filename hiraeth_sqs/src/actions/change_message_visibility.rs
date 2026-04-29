use std::collections::HashMap;

use async_trait::async_trait;
use chrono::{Duration, Utc};
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

pub(crate) struct ChangeMessageVisibilityAction;

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub(crate) struct ChangeMessageVisibilityRequest {
    pub queue_url: String,
    pub receipt_handle: String,
    pub visibility_timeout: u32,
}

async fn handle_change_message_visibility_typed<S: SqsStore>(
    request: &ResolvedRequest,
    store: &S,
    change_request: ChangeMessageVisibilityRequest,
) -> Result<(), SqsError> {
    let queue = crate::util::load_queue_from_url(request, store, &change_request.queue_url).await?;
    validate_visibility_timeout(change_request.visibility_timeout)?;

    store
        .set_message_visible_at(
            queue.id,
            &change_request.receipt_handle,
            (Utc::now() + Duration::seconds(change_request.visibility_timeout as i64)).naive_utc(),
        )
        .await
        .map_err(crate::error::map_receipt_handle_store_error)?;

    Ok(())
}

pub(super) fn validate_visibility_timeout(visibility_timeout: u32) -> Result<(), SqsError> {
    if visibility_timeout > 43200 {
        return Err(SqsError::BadRequest(
            "VisibilityTimeout must be between 0 and 43200".to_string(),
        ));
    }

    Ok(())
}

#[async_trait]
impl<S> TypedAwsAction<S> for ChangeMessageVisibilityAction
where
    S: SqsStore + Send + Sync,
{
    type Request = ChangeMessageVisibilityRequest;
    type Response = ();
    type Error = SqsError;

    fn name(&self) -> &'static str {
        "ChangeMessageVisibility"
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

    async fn validate(
        &self,
        _request: &ResolvedRequest,
        change_request: &ChangeMessageVisibilityRequest,
        _store: &S,
    ) -> Result<(), SqsError> {
        validate_visibility_timeout(change_request.visibility_timeout)
    }

    async fn handle(
        &self,
        request: ResolvedRequest,
        change_request: ChangeMessageVisibilityRequest,
        store: &S,
        trace_context: &TraceContext,
        trace_recorder: &dyn TraceRecorder,
    ) -> Result<(), SqsError> {
        let timer = trace_context.start_span();
        let attributes = HashMap::from([
            ("queue_url".to_string(), change_request.queue_url.clone()),
            (
                "receipt_handle".to_string(),
                change_request.receipt_handle.clone(),
            ),
            (
                "visibility_timeout_seconds".to_string(),
                change_request.visibility_timeout.to_string(),
            ),
        ]);

        let result = handle_change_message_visibility_typed(&request, store, change_request).await;
        let status = if result.is_ok() { "ok" } else { "error" };
        trace_context
            .record_span_or_warn(
                trace_recorder,
                timer,
                "sqs.message.change_visibility",
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
        _payload: ChangeMessageVisibilityRequest,
        store: &S,
    ) -> Result<AuthorizationCheck, SqsError> {
        crate::auth::resolve_authorization("sqs:ChangeMessageVisibility", request, store).await
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use chrono::{Duration, TimeZone, Utc};
    use hiraeth_core::{AuthContext, ResolvedRequest, TypedAwsAction};
    use hiraeth_http::IncomingRequest;
    use hiraeth_store::{principal::Principal, sqs::SqsQueue, test_support::SqsTestStore};

    use super::{ChangeMessageVisibilityAction, handle_change_message_visibility_typed};
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

    #[test]
    fn reports_expected_action_name() {
        assert_eq!(
            <ChangeMessageVisibilityAction as TypedAwsAction<SqsTestStore>>::name(
                &ChangeMessageVisibilityAction
            ),
            "ChangeMessageVisibility"
        );
    }

    #[tokio::test]
    async fn updates_visible_at_for_receipt_handle() {
        let store = SqsTestStore::with_queue(queue());
        let request = resolved_request(
            r#"{
                "QueueUrl":"http://localhost:4566/123456789012/orders",
                "ReceiptHandle":"receipt-123",
                "VisibilityTimeout":45
            }"#,
        );

        let before = Utc::now().naive_utc();
        handle_change_message_visibility_typed(
            &request,
            &store,
            crate::actions::test_support::parse_request_body(&request),
        )
        .await
        .expect("change message visibility should succeed");
        let after = Utc::now().naive_utc();
        let updates = store.visibility_updates();
        assert_eq!(updates.len(), 1);
        assert_eq!(updates[0].0, 42);
        assert_eq!(updates[0].1, "receipt-123");
        assert!(updates[0].2 >= before + Duration::seconds(45));
        assert!(updates[0].2 <= after + Duration::seconds(45));
    }

    #[tokio::test]
    async fn returns_not_found_for_missing_queue() {
        let store = SqsTestStore::default();
        let request = resolved_request(
            r#"{
                "QueueUrl":"http://localhost:4566/123456789012/orders",
                "ReceiptHandle":"receipt-123",
                "VisibilityTimeout":45
            }"#,
        );

        let error = handle_change_message_visibility_typed(
            &request,
            &store,
            crate::actions::test_support::parse_request_body(&request),
        )
        .await
        .expect_err("missing queue should error");

        assert_eq!(error, SqsError::QueueNotFound);
    }
}
