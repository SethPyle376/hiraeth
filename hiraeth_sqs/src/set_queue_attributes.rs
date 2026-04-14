use std::collections::HashMap;

use hiraeth_auth::ResolvedRequest;
use hiraeth_router::ServiceResponse;
use hiraeth_store::sqs::{SqsQueueAttributeUpdate, SqsStore};
use serde::Deserialize;

use crate::{error::SqsError, util};

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct SetQueueAttributesRequest {
    pub queue_url: String,
    pub attributes: HashMap<String, String>,
}

pub(crate) async fn set_queue_attributes<S: SqsStore>(
    request: &ResolvedRequest,
    store: &S,
) -> Result<ServiceResponse, SqsError> {
    let request_body = util::parse_request_body::<SetQueueAttributesRequest>(request)?;
    let queue = util::load_queue_from_url(request, store, request_body.queue_url.as_str()).await?;

    store
        .set_queue_attributes(
            queue.id,
            SqsQueueAttributeUpdate::from(request_body.attributes),
        )
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
    use hiraeth_store::{
        principal::Principal,
        sqs::{SqsQueue, SqsStore},
        test_support::SqsTestStore,
    };

    use super::set_queue_attributes;
    use crate::error::SqsError;

    fn resolved_request(body: &str) -> ResolvedRequest {
        let mut headers = HashMap::new();
        headers.insert(
            "x-amz-target".to_string(),
            "AmazonSQS.SetQueueAttributes".to_string(),
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

    #[tokio::test]
    async fn set_queue_attributes_updates_requested_attributes_and_preserves_omitted_values() {
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

        let response = set_queue_attributes(&request, &store)
            .await
            .expect("set queue attributes should succeed");

        assert_eq!(response.status_code, 200);
        assert!(response.body.is_empty());

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
    async fn set_queue_attributes_returns_not_found_for_missing_queue() {
        let store = SqsTestStore::default();
        let request = resolved_request(
            r#"{
                "QueueUrl":"http://localhost:4566/123456789012/orders",
                "Attributes":{"VisibilityTimeout":"45"}
            }"#,
        );

        let result = set_queue_attributes(&request, &store).await;

        assert!(matches!(result, Err(SqsError::QueueNotFound)));
    }
}
