use std::collections::HashMap;

use async_trait::async_trait;
use hiraeth_core::{
    ApiError, AwsAction, ResolvedRequest, ServiceResponse, auth::AuthorizationCheck, empty_response,
};
use hiraeth_store::sqs::{SqsQueue, SqsStore};
use serde::Deserialize;

use super::tag_support::validate_tags;
use crate::error::SqsError;

pub(crate) struct TagQueueAction;

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct TagQueueRequest {
    queue_url: String,
    tags: HashMap<String, String>,
}

async fn handle_tag_queue<S: SqsStore>(
    request: &ResolvedRequest,
    store: &S,
) -> Result<ServiceResponse, SqsError> {
    let request_body = crate::util::parse_request_body::<TagQueueRequest>(request)?;
    validate_tags(&request_body.tags, false)?;

    let queue = crate::util::load_queue_from_url(request, store, &request_body.queue_url).await?;
    let mut merged_tags = store
        .list_queue_tags(queue.id)
        .await
        .map_err(crate::error::map_store_error)?;
    merged_tags.extend(request_body.tags.clone());
    validate_tags(&merged_tags, true)?;

    store
        .tag_queue(queue.id, request_body.tags)
        .await
        .map(|_| empty_response())
        .map_err(crate::error::map_store_error)
}

#[async_trait]
impl<S> AwsAction<S> for TagQueueAction
where
    S: SqsStore + Send + Sync,
{
    fn name(&self) -> &'static str {
        "TagQueue"
    }

    async fn handle(
        &self,
        request: ResolvedRequest,
        store: &S,
    ) -> Result<ServiceResponse, ApiError> {
        match handle_tag_queue(&request, store).await {
            Ok(response) => Ok(response),
            Err(error) => Ok(ServiceResponse::from(error)),
        }
    }

    async fn resolve_authorization(
        &self,
        request: &ResolvedRequest,
        store: &S,
    ) -> Result<AuthorizationCheck, ServiceResponse> {
        crate::auth::resolve_authorization("sqs:TagQueue", request, store).await
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use chrono::{TimeZone, Utc};
    use hiraeth_core::{AuthContext, AwsAction, ResolvedRequest};
    use hiraeth_http::IncomingRequest;
    use hiraeth_store::{
        principal::Principal, sqs::SqsQueue, sqs::SqsStore, test_support::SqsTestStore,
    };

    use super::{TagQueueAction, handle_tag_queue};
    use crate::error::SqsError;

    fn resolved_request(body: &str) -> ResolvedRequest {
        ResolvedRequest {
            request: IncomingRequest {
                host: "localhost:4566".to_string(),
                method: "POST".to_string(),
                path: "/".to_string(),
                query: None,
                headers: [("x-amz-target".to_string(), "AmazonSQS.TagQueue".to_string())]
                    .into_iter()
                    .collect(),
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
                        .with_ymd_and_hms(2026, 4, 15, 12, 0, 0)
                        .unwrap()
                        .naive_utc(),
                },
            },
            date: Utc.with_ymd_and_hms(2026, 4, 15, 12, 0, 0).unwrap(),
        }
    }

    fn queue() -> SqsQueue {
        SqsQueue {
            id: 42,
            name: "orders".to_string(),
            region: "us-east-1".to_string(),
            account_id: "123456789012".to_string(),
            created_at: Utc
                .with_ymd_and_hms(2026, 4, 15, 12, 0, 0)
                .unwrap()
                .naive_utc(),
            updated_at: Utc
                .with_ymd_and_hms(2026, 4, 15, 12, 0, 0)
                .unwrap()
                .naive_utc(),
            ..Default::default()
        }
    }

    #[test]
    fn reports_expected_action_name() {
        assert_eq!(
            <TagQueueAction as AwsAction<SqsTestStore>>::name(&TagQueueAction),
            "TagQueue"
        );
    }

    #[tokio::test]
    async fn merges_with_existing_tags() {
        let store = SqsTestStore::with_queue(queue());
        store
            .tag_queue(
                42,
                [("environment".to_string(), "test".to_string())]
                    .into_iter()
                    .collect(),
            )
            .await
            .expect("tags should seed");
        let request = resolved_request(
            r#"{
                "QueueUrl":"http://localhost:4566/123456789012/orders",
                "Tags":{
                    "owner":"hiraeth",
                    "environment":"prod"
                }
            }"#,
        );

        let response = handle_tag_queue(&request, &store)
            .await
            .expect("tag queue should succeed");

        assert_eq!(response.status_code, 200);
        assert_eq!(
            store.queue_tags(42),
            [
                ("environment".to_string(), "prod".to_string()),
                ("owner".to_string(), "hiraeth".to_string()),
            ]
            .into_iter()
            .collect::<HashMap<_, _>>()
        );
    }

    #[tokio::test]
    async fn rejects_reserved_tag_key_prefix() {
        let store = SqsTestStore::with_queue(queue());
        let request = resolved_request(
            r#"{
                "QueueUrl":"http://localhost:4566/123456789012/orders",
                "Tags":{"aws:reserved":"value"}
            }"#,
        );

        let result = handle_tag_queue(&request, &store).await;

        assert!(matches!(result, Err(SqsError::BadRequest(_))));
    }

    #[tokio::test]
    async fn rejects_more_than_fifty_total_tags() {
        let store = SqsTestStore::with_queue(queue());
        let existing_tags = (0..49)
            .map(|index| (format!("existing-{index}"), "value".to_string()))
            .collect();
        store
            .tag_queue(42, existing_tags)
            .await
            .expect("tags should seed");
        let request = resolved_request(
            r#"{
                "QueueUrl":"http://localhost:4566/123456789012/orders",
                "Tags":{"extra-1":"value","extra-2":"value"}
            }"#,
        );

        let result = handle_tag_queue(&request, &store).await;

        assert!(matches!(result, Err(SqsError::BadRequest(_))));
    }

    #[tokio::test]
    async fn returns_not_found_for_missing_queue() {
        let store = SqsTestStore::default();
        let request = resolved_request(
            r#"{
                "QueueUrl":"http://localhost:4566/123456789012/orders",
                "Tags":{"environment":"test"}
            }"#,
        );

        let result = handle_tag_queue(&request, &store).await;

        assert!(matches!(result, Err(SqsError::QueueNotFound)));
    }
}
