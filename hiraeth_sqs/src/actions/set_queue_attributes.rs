use std::collections::HashMap;

use hiraeth_core::{ResolvedRequest, TypedAwsAction, impl_aws_action};
use hiraeth_core::tracing::{TraceContext, TraceRecorder};
use hiraeth_store::sqs::{SqsQueue, SqsStore};
use serde::Deserialize;

use super::{
    action_support::parse_payload_error,
    queue_attribute_support::parse_queue_attribute_update,
};
use crate::error::SqsError;

pub(crate) struct SetQueueAttributesAction;

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub(crate) struct SetQueueAttributesRequest {
    pub queue_url: String,
    pub attributes: HashMap<String, String>,
}

async fn handle_set_queue_attributes_typed<S: SqsStore>(
    request: &ResolvedRequest,
    store: &S,
    request_body: SetQueueAttributesRequest,
) -> Result<(), SqsError> {
    let queue =
        crate::util::load_queue_from_url(request, store, request_body.queue_url.as_str()).await?;

    store
        .set_queue_attributes(
            queue.id,
            parse_queue_attribute_update(&request_body.attributes)?,
        )
        .await
        .map_err(crate::error::map_store_error)
}

impl_aws_action! {
    SetQueueAttributesAction<S: SqsStore> {
        request: SetQueueAttributesRequest,
        response: (),
        error: SqsError,
        name: "SetQueueAttributes",
        payload: Json,
        response_format: Empty,
        parse_error: parse_payload_error,
        validate: |_request, request_body, _store| {
            parse_queue_attribute_update(&request_body.attributes)?;
            Ok(())
        },
        handle: |request, payload, store, trace_context, trace_recorder| {
            let attributes = HashMap::from([
                ("queue_url".to_string(), payload.queue_url.clone()),
                (
                    "attribute_count".to_string(),
                    payload.attributes.len().to_string(),
                ),
                (
                    "attributes".to_string(),
                    payload
                        .attributes
                        .keys()
                        .cloned()
                        .collect::<Vec<_>>()
                        .join(","),
                ),
            ]);

            trace_context
                .record_result_span(
                    trace_recorder,
                    "sqs.queue.set_attributes",
                    "sqs",
                    attributes,
                    async { handle_set_queue_attributes_typed(&request, store, payload).await },
                )
                .await
        },
        authorize: |request, _payload, store| {
            crate::auth::resolve_authorization("sqs:SetQueueAttributes", request, store).await
        },
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use chrono::{TimeZone, Utc};
    use hiraeth_core::{AuthContext, ResolvedRequest, TypedAwsAction};
    use hiraeth_http::IncomingRequest;
    use hiraeth_store::{
        principal::Principal,
        sqs::{SqsQueue, SqsStore},
        test_support::SqsTestStore,
    };

    use super::{SetQueueAttributesAction, handle_set_queue_attributes_typed};
    use crate::error::SqsError;

    fn resolved_request(body: &str) -> ResolvedRequest {
        let mut headers = HashMap::new();
        headers.insert(
            "x-amz-target".to_string(),
            "AmazonSQS.SetQueueAttributes".to_string(),
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
                        .with_ymd_and_hms(2026, 4, 14, 12, 0, 0)
                        .unwrap()
                        .naive_utc(),
                },
            },
            date: Utc.with_ymd_and_hms(2026, 4, 14, 12, 0, 0).unwrap(),
        }
    }

    fn queue() -> SqsQueue {
        SqsQueue {
            id: 42,
            name: "orders".to_string(),
            region: "us-east-1".to_string(),
            account_id: "123456789012".to_string(),
            delay_seconds: 5,
            policy: r#"{"Statement":[]}"#.to_string(),
            kms_master_key_id: Some("alias/original".to_string()),
            updated_at: Utc
                .with_ymd_and_hms(2026, 4, 14, 11, 0, 0)
                .unwrap()
                .naive_utc(),
            ..Default::default()
        }
    }

    #[test]
    fn reports_expected_action_name() {
        assert_eq!(
            <SetQueueAttributesAction as TypedAwsAction<SqsTestStore>>::name(
                &SetQueueAttributesAction
            ),
            "SetQueueAttributes"
        );
    }

    #[tokio::test]
    async fn updates_requested_attributes_and_preserves_omitted_values() {
        let store = SqsTestStore::with_queue(queue());
        let request = resolved_request(
            r#"{
                "QueueUrl":"http://localhost:4566/123456789012/orders",
                "Attributes":{
                    "VisibilityTimeout":"45",
                    "Policy":"{\"Version\":\"2012-10-17\"}",
                    "SqsManagedSseEnabled":"true"
                }
            }"#,
        );

        handle_set_queue_attributes_typed(
            &request,
            &store,
            crate::actions::test_support::parse_request_body(&request),
        )
        .await
        .expect("set queue attributes should succeed");

        let updated_queue = store
            .get_queue("orders", "us-east-1", "123456789012")
            .await
            .expect("queue lookup should succeed")
            .expect("queue should exist");

        assert_eq!(updated_queue.visibility_timeout_seconds, 45);
        assert_eq!(updated_queue.policy, r#"{"Version":"2012-10-17"}"#);
        assert!(updated_queue.sqs_managed_sse_enabled);
        assert_eq!(updated_queue.delay_seconds, 5);
        assert_eq!(
            updated_queue.kms_master_key_id.as_deref(),
            Some("alias/original")
        );
    }

    #[tokio::test]
    async fn returns_not_found_for_missing_queue() {
        let store = SqsTestStore::default();
        let request = resolved_request(
            r#"{
                "QueueUrl":"http://localhost:4566/123456789012/orders",
                "Attributes":{"VisibilityTimeout":"45"}
            }"#,
        );

        let result = handle_set_queue_attributes_typed(
            &request,
            &store,
            crate::actions::test_support::parse_request_body(&request),
        )
        .await;

        assert!(matches!(result, Err(SqsError::QueueNotFound)));
    }

    #[tokio::test]
    async fn rejects_invalid_attribute_values() {
        let store = SqsTestStore::with_queue(queue());
        let request = resolved_request(
            r#"{
                "QueueUrl":"http://localhost:4566/123456789012/orders",
                "Attributes":{"VisibilityTimeout":"not-a-number"}
            }"#,
        );

        let result = handle_set_queue_attributes_typed(
            &request,
            &store,
            crate::actions::test_support::parse_request_body(&request),
        )
        .await;

        assert!(matches!(result, Err(SqsError::BadRequest(_))));
    }
}
