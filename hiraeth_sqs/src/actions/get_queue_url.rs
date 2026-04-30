use std::collections::HashMap;

use async_trait::async_trait;
use hiraeth_core::{
    AwsActionPayloadFormat, AwsActionPayloadParseError, ResolvedRequest, TypedAwsAction,
    auth::AuthorizationCheck,
    tracing::{TraceContext, TraceRecorder},
};
use hiraeth_store::sqs::SqsStore;
use serde::{Deserialize, Serialize};

use super::{
    action_support::{json_payload_format, parse_payload_error},
    queue_support,
};
use crate::error::SqsError;

pub(crate) struct GetQueueUrlAction;

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub(crate) struct GetQueueUrlRequest {
    pub(crate) queue_name: String,
    pub(crate) queue_owner_aws_account_id: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "PascalCase")]
pub(crate) struct GetQueueUrlResponse {
    queue_url: String,
}

async fn handle_get_queue_url_typed<S: SqsStore>(
    request: &ResolvedRequest,
    store: &S,
    request_body: GetQueueUrlRequest,
) -> Result<GetQueueUrlResponse, SqsError> {
    queue_support::validate_queue_name(
        &request_body.queue_name,
        request_body.queue_name.ends_with(".fifo"),
    )?;

    let account_id = request_body
        .queue_owner_aws_account_id
        .unwrap_or_else(|| request.auth_context.principal.account_id.clone());

    let queue = store
        .get_queue(&request_body.queue_name, &request.region, &account_id)
        .await
        .map_err(|e| SqsError::InternalError(e.to_string()))?;

    match queue {
        Some(_) => {
            let response = GetQueueUrlResponse {
                queue_url: crate::util::queue_url(
                    &request.request.host,
                    &account_id,
                    &request_body.queue_name,
                ),
            };
            Ok(response)
        }
        None => Err(SqsError::QueueNotFound),
    }
}

#[async_trait]
impl<S> TypedAwsAction<S> for GetQueueUrlAction
where
    S: SqsStore + Send + Sync,
{
    type Request = GetQueueUrlRequest;
    type Response = GetQueueUrlResponse;
    type Error = SqsError;

    fn name(&self) -> &'static str {
        "GetQueueUrl"
    }

    fn payload_format(&self) -> AwsActionPayloadFormat {
        json_payload_format()
    }

    fn parse_error(&self, error: AwsActionPayloadParseError) -> SqsError {
        parse_payload_error(error)
    }

    async fn validate(
        &self,
        _request: &ResolvedRequest,
        request_body: &GetQueueUrlRequest,
        _store: &S,
    ) -> Result<(), SqsError> {
        queue_support::validate_queue_name(
            &request_body.queue_name,
            request_body.queue_name.ends_with(".fifo"),
        )
    }

    async fn handle(
        &self,
        request: ResolvedRequest,
        request_body: GetQueueUrlRequest,
        store: &S,
        trace_context: &TraceContext,
        trace_recorder: &dyn TraceRecorder,
    ) -> Result<GetQueueUrlResponse, SqsError> {
        let attributes = HashMap::from([
            ("queue_name".to_string(), request_body.queue_name.clone()),
            ("region".to_string(), request.region.clone()),
            (
                "queue_owner_account_id".to_string(),
                request_body
                    .queue_owner_aws_account_id
                    .clone()
                    .unwrap_or_else(|| request.auth_context.principal.account_id.clone()),
            ),
        ]);

        trace_context
            .record_result_span(
                trace_recorder,
                "sqs.queue.lookup_url",
                "sqs",
                attributes,
                async { handle_get_queue_url_typed(&request, store, request_body).await },
            )
            .await
    }

    async fn resolve_authorization_typed(
        &self,
        request: &ResolvedRequest,
        _payload: GetQueueUrlRequest,
        store: &S,
    ) -> Result<AuthorizationCheck, SqsError> {
        crate::auth::resolve_authorization("sqs:GetQueueUrl", request, store).await
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use chrono::{TimeZone, Utc};
    use hiraeth_core::{AuthContext, ResolvedRequest, TypedAwsAction};
    use hiraeth_http::IncomingRequest;
    use hiraeth_store::{principal::Principal, sqs::SqsQueue, test_support::SqsTestStore};

    use super::{GetQueueUrlAction, handle_get_queue_url_typed};
    use crate::error::SqsError;

    fn resolved_request(body: &str) -> ResolvedRequest {
        let mut headers = HashMap::new();
        headers.insert(
            "x-amz-target".to_string(),
            "AmazonSQS.GetQueueUrl".to_string(),
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
                        .with_ymd_and_hms(2026, 4, 1, 12, 0, 0)
                        .unwrap()
                        .naive_utc(),
                },
            },
            date: Utc.with_ymd_and_hms(2026, 4, 1, 12, 0, 0).unwrap(),
        }
    }

    fn aws_style_resolved_request(body: &str) -> ResolvedRequest {
        let mut request = resolved_request(body);
        request.request.host = "sqs.us-east-1.amazonaws.com".to_string();
        request
    }

    #[test]
    fn reports_expected_action_name() {
        assert_eq!(
            <GetQueueUrlAction as TypedAwsAction<SqsTestStore>>::name(&GetQueueUrlAction),
            "GetQueueUrl"
        );
    }

    #[tokio::test]
    async fn returns_not_found_when_queue_does_not_exist() {
        let store = SqsTestStore::default();
        let request = resolved_request(r#"{"QueueName":"missing-queue"}"#);

        let result = handle_get_queue_url_typed(
            &request,
            &store,
            crate::actions::test_support::parse_request_body(&request),
        )
        .await;

        assert!(matches!(result, Err(SqsError::QueueNotFound)));
    }

    #[tokio::test]
    async fn returns_queue_url_when_queue_exists() {
        let store = SqsTestStore::with_queue(SqsQueue {
            id: 1,
            name: "existing-queue".to_string(),
            region: "us-east-1".to_string(),
            account_id: "123456789012".to_string(),
            queue_type: "standard".to_string(),
            visibility_timeout_seconds: 30,
            delay_seconds: 0,
            message_retention_period_seconds: 345600,
            receive_message_wait_time_seconds: 0,
            created_at: Utc
                .with_ymd_and_hms(2026, 4, 1, 12, 0, 0)
                .unwrap()
                .naive_utc(),
            updated_at: Utc
                .with_ymd_and_hms(2026, 4, 1, 12, 0, 0)
                .unwrap()
                .naive_utc(),
            ..Default::default()
        });
        let request = resolved_request(r#"{"QueueName":"existing-queue"}"#);

        let response = handle_get_queue_url_typed(
            &request,
            &store,
            crate::actions::test_support::parse_request_body(&request),
        )
        .await
        .expect("get queue url should succeed");
        let body = serde_json::to_value(response).expect("response should serialize to json");
        assert_eq!(
            body["QueueUrl"],
            "http://localhost:4566/123456789012/existing-queue"
        );
    }

    #[tokio::test]
    async fn returns_aws_style_queue_url_for_aws_hostnames() {
        let store = SqsTestStore::with_queue(SqsQueue {
            id: 1,
            name: "existing-queue".to_string(),
            region: "us-east-1".to_string(),
            account_id: "123456789012".to_string(),
            queue_type: "standard".to_string(),
            visibility_timeout_seconds: 30,
            delay_seconds: 0,
            message_retention_period_seconds: 345600,
            receive_message_wait_time_seconds: 0,
            created_at: Utc
                .with_ymd_and_hms(2026, 4, 1, 12, 0, 0)
                .unwrap()
                .naive_utc(),
            updated_at: Utc
                .with_ymd_and_hms(2026, 4, 1, 12, 0, 0)
                .unwrap()
                .naive_utc(),
            ..Default::default()
        });
        let request = aws_style_resolved_request(r#"{"QueueName":"existing-queue"}"#);

        let response = handle_get_queue_url_typed(
            &request,
            &store,
            crate::actions::test_support::parse_request_body(&request),
        )
        .await
        .expect("get queue url should succeed");
        let body = serde_json::to_value(response).expect("response should serialize to json");

        assert_eq!(
            body["QueueUrl"],
            "http://sqs.us-east-1.amazonaws.com/123456789012/existing-queue"
        );
    }
}
