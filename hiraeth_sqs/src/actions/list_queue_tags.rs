use std::collections::HashMap;

use async_trait::async_trait;
use hiraeth_core::{
    ApiError, AwsAction, ResolvedRequest, ServiceResponse, auth::AuthorizationCheck, json_response,
};
use hiraeth_store::sqs::{SqsQueue, SqsStore};
use serde::{Deserialize, Serialize};

use crate::error::SqsError;

pub(crate) struct ListQueueTagsAction;

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct ListQueueTagsRequest {
    queue_url: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "PascalCase")]
struct ListQueueTagsResponse {
    tags: HashMap<String, String>,
}

async fn handle_list_queue_tags<S: SqsStore>(
    request: &ResolvedRequest,
    store: &S,
) -> Result<ServiceResponse, SqsError> {
    let request_body = crate::util::parse_request_body::<ListQueueTagsRequest>(request)?;
    let queue = crate::util::load_queue_from_url(request, store, &request_body.queue_url).await?;

    let tags = store
        .list_queue_tags(queue.id)
        .await
        .map_err(crate::error::map_store_error)?;

    json_response(&ListQueueTagsResponse { tags }).map_err(Into::into)
}

#[async_trait]
impl<S> AwsAction<S> for ListQueueTagsAction
where
    S: SqsStore + Send + Sync,
{
    fn name(&self) -> &'static str {
        "ListQueueTags"
    }

    async fn handle(
        &self,
        request: ResolvedRequest,
        store: &S,
    ) -> Result<ServiceResponse, ApiError> {
        match handle_list_queue_tags(&request, store).await {
            Ok(response) => Ok(response),
            Err(error) => Ok(ServiceResponse::from(error)),
        }
    }

    async fn resolve_authorization(
        &self,
        request: &ResolvedRequest,
        store: &S,
    ) -> Result<AuthorizationCheck, ServiceResponse> {
        crate::auth::resolve_authorization("sqs:ListQueueTags", request, store).await
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use chrono::{TimeZone, Utc};
    use hiraeth_core::{AuthContext, AwsAction, ResolvedRequest};
    use hiraeth_http::IncomingRequest;
    use hiraeth_router::ServiceResponse;
    use hiraeth_store::{
        principal::Principal, sqs::SqsQueue, sqs::SqsStore, test_support::SqsTestStore,
    };

    use super::{ListQueueTagsAction, handle_list_queue_tags};

    fn resolved_request(body: &str) -> ResolvedRequest {
        ResolvedRequest {
            request: IncomingRequest {
                host: "localhost:4566".to_string(),
                method: "POST".to_string(),
                path: "/".to_string(),
                query: None,
                headers: [(
                    "x-amz-target".to_string(),
                    "AmazonSQS.ListQueueTags".to_string(),
                )]
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
                    path: "/".to_string(),
                    user_id: "AIDATESTUSER000001".to_string(),
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

    fn parse_json_body(response: &ServiceResponse) -> serde_json::Value {
        serde_json::from_slice(&response.body).expect("response body should be valid json")
    }

    #[test]
    fn reports_expected_action_name() {
        assert_eq!(
            <ListQueueTagsAction as AwsAction<SqsTestStore>>::name(&ListQueueTagsAction),
            "ListQueueTags"
        );
    }

    #[tokio::test]
    async fn returns_existing_tags() {
        let store = SqsTestStore::with_queue(queue());
        store
            .tag_queue(
                42,
                [
                    ("environment".to_string(), "test".to_string()),
                    ("owner".to_string(), "hiraeth".to_string()),
                ]
                .into_iter()
                .collect(),
            )
            .await
            .expect("tags should seed");
        let request =
            resolved_request(r#"{"QueueUrl":"http://localhost:4566/123456789012/orders"}"#);

        let response = handle_list_queue_tags(&request, &store)
            .await
            .expect("list queue tags should succeed");
        let body = parse_json_body(&response);

        assert_eq!(response.status_code, 200);
        assert_eq!(body["Tags"]["environment"], "test");
        assert_eq!(body["Tags"]["owner"], "hiraeth");
    }
}
