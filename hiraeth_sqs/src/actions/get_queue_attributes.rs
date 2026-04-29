use std::collections::HashMap;

use async_trait::async_trait;
use hiraeth_core::{
    AwsActionPayloadFormat, AwsActionPayloadParseError, ResolvedRequest, ServiceResponse,
    TypedAwsAction, auth::AuthorizationCheck, json_response,
};
use hiraeth_store::sqs::{SqsQueue, SqsStore};
use serde::{Deserialize, Serialize};

use super::{
    action_support::{json_payload_format, parse_payload_error},
    queue_attribute_support::collect_queue_attributes,
};
use crate::error::SqsError;

pub(crate) struct GetQueueAttributesAction;

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct GetQueueAttributesRequest {
    pub queue_url: String,
    pub attribute_names: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "PascalCase")]
pub struct GetQueueAttributesResponse {
    pub attributes: HashMap<String, String>,
}

async fn handle_get_queue_attributes_typed<S: SqsStore>(
    request: &ResolvedRequest,
    store: &S,
    attributes_request: GetQueueAttributesRequest,
) -> Result<ServiceResponse, SqsError> {
    let queue =
        crate::util::load_queue_from_url(request, store, &attributes_request.queue_url).await?;
    let attributes =
        collect_queue_attributes(store, &queue, &attributes_request.attribute_names).await?;

    json_response(&GetQueueAttributesResponse { attributes }).map_err(Into::into)
}

#[async_trait]
impl<S> TypedAwsAction<S> for GetQueueAttributesAction
where
    S: SqsStore + Send + Sync,
{
    type Request = GetQueueAttributesRequest;
    type Error = SqsError;

    fn name(&self) -> &'static str {
        "GetQueueAttributes"
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
        attributes_request: GetQueueAttributesRequest,
        store: &S,
        trace_context: &hiraeth_core::tracing::TraceContext,
        trace_recorder: &dyn hiraeth_core::tracing::TraceRecorder,
    ) -> Result<ServiceResponse, SqsError> {
        let timer = trace_context.start_span();
        let attributes = HashMap::from([
            (
                "queue_url".to_string(),
                attributes_request.queue_url.clone(),
            ),
            (
                "requested_attribute_count".to_string(),
                attributes_request.attribute_names.len().to_string(),
            ),
            (
                "requested_attributes".to_string(),
                attributes_request.attribute_names.join(","),
            ),
        ]);

        let result = handle_get_queue_attributes_typed(&request, store, attributes_request).await;
        let status = if result.is_ok() { "ok" } else { "error" };
        trace_context
            .record_span_or_warn(
                trace_recorder,
                timer,
                "sqs.queue.get_attributes",
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
        _payload: GetQueueAttributesRequest,
        store: &S,
    ) -> Result<AuthorizationCheck, SqsError> {
        crate::auth::resolve_authorization("sqs:GetQueueAttributes", request, store).await
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

    use super::{GetQueueAttributesAction, handle_get_queue_attributes_typed};
    use crate::error::SqsError;

    fn resolved_request(body: &str) -> ResolvedRequest {
        let mut headers = HashMap::new();
        headers.insert(
            "x-amz-target".to_string(),
            "AmazonSQS.GetQueueAttributes".to_string(),
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
            delay_seconds: 5,
            maximum_message_size: 2048,
            message_retention_period_seconds: 345600,
            receive_message_wait_time_seconds: 10,
            policy: r#"{"Statement":[]}"#.to_string(),
            redrive_policy: r#"{"maxReceiveCount":"5"}"#.to_string(),
            content_based_deduplication: true,
            kms_master_key_id: Some("alias/test".to_string()),
            kms_data_key_reuse_period_seconds: 600,
            deduplication_scope: "messageGroup".to_string(),
            fifo_throughput_limit: "perMessageGroupId".to_string(),
            redrive_allow_policy: r#"{"redrivePermission":"allowAll"}"#.to_string(),
            sqs_managed_sse_enabled: true,
            created_at: Utc
                .with_ymd_and_hms(2026, 4, 4, 11, 0, 0)
                .unwrap()
                .naive_utc(),
            updated_at: Utc
                .with_ymd_and_hms(2026, 4, 4, 11, 30, 0)
                .unwrap()
                .naive_utc(),
        }
    }

    fn parse_json_body(response: &ServiceResponse) -> Value {
        serde_json::from_slice(&response.body).expect("response body should be valid json")
    }

    #[test]
    fn reports_expected_action_name() {
        assert_eq!(
            <GetQueueAttributesAction as TypedAwsAction<SqsTestStore>>::name(
                &GetQueueAttributesAction
            ),
            "GetQueueAttributes"
        );
    }

    #[tokio::test]
    async fn returns_requested_attributes() {
        let store = SqsTestStore::with_queue(queue()).with_message_counts(7, 3, 2);
        let request = resolved_request(
            r#"{
                "QueueUrl":"http://localhost:4566/123456789012/orders",
                "AttributeNames":[
                    "VisibilityTimeout",
                    "ApproximateNumberOfMessages",
                    "ApproximateNumberOfMessagesNotVisible",
                    "ApproximateNumberOfMessagesDelayed",
                    "QueueArn",
                    "CreatedTimestamp",
                    "ReceiveMessageWaitTimeSeconds"
                ]
            }"#,
        );

        let response = handle_get_queue_attributes_typed(
            &request,
            &store,
            crate::actions::test_support::parse_request_body(&request),
        )
        .await
        .expect("get queue attributes should succeed");

        assert_eq!(response.status_code, 200);
        let body = parse_json_body(&response);
        let attributes = &body["Attributes"];
        assert_eq!(attributes["VisibilityTimeout"], "30");
        assert_eq!(attributes["ApproximateNumberOfMessages"], "7");
        assert_eq!(attributes["ApproximateNumberOfMessagesNotVisible"], "4");
        assert_eq!(attributes["ApproximateNumberOfMessagesDelayed"], "2");
        assert_eq!(
            attributes["QueueArn"],
            "arn:aws:sqs:us-east-1:123456789012:orders"
        );
        assert_eq!(
            attributes["CreatedTimestamp"],
            Utc.with_ymd_and_hms(2026, 4, 4, 11, 0, 0)
                .unwrap()
                .timestamp_millis()
                .to_string()
        );
        assert_eq!(attributes["ReceiveMessageWaitTimeSeconds"], "10");
    }

    #[tokio::test]
    async fn returns_not_found_for_missing_queue() {
        let store = SqsTestStore::default();
        let request = resolved_request(
            r#"{
                "QueueUrl":"http://localhost:4566/123456789012/orders",
                "AttributeNames":["All"]
            }"#,
        );

        let result = handle_get_queue_attributes_typed(
            &request,
            &store,
            crate::actions::test_support::parse_request_body(&request),
        )
        .await;

        assert!(matches!(result, Err(SqsError::QueueNotFound)));
    }
}
