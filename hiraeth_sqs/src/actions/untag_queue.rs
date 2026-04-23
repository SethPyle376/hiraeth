use async_trait::async_trait;
use hiraeth_core::{
    ApiError, AwsAction, ResolvedRequest, ServiceResponse, auth::AuthorizationCheck, empty_response,
};
use hiraeth_store::sqs::SqsStore;
use serde::Deserialize;

use super::tag_support::validate_tag_keys;
use crate::error::SqsError;

pub(crate) struct UntagQueueAction;

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct UntagQueueRequest {
    queue_url: String,
    tag_keys: Vec<String>,
}

async fn handle_untag_queue<S: SqsStore>(
    request: &ResolvedRequest,
    store: &S,
) -> Result<ServiceResponse, SqsError> {
    let request_body = crate::util::parse_request_body::<UntagQueueRequest>(request)?;
    validate_tag_keys(&request_body.tag_keys, false)?;

    let queue = crate::util::load_queue_from_url(request, store, &request_body.queue_url).await?;
    store
        .untag_queue(queue.id, request_body.tag_keys)
        .await
        .map(|_| empty_response())
        .map_err(crate::error::map_store_error)
}

#[async_trait]
impl<S> AwsAction<S> for UntagQueueAction
where
    S: SqsStore + Send + Sync,
{
    fn name(&self) -> &'static str {
        "UntagQueue"
    }

    async fn handle(
        &self,
        request: ResolvedRequest,
        store: &S,
    ) -> Result<ServiceResponse, ApiError> {
        match handle_untag_queue(&request, store).await {
            Ok(response) => Ok(response),
            Err(error) => Ok(ServiceResponse::from(error)),
        }
    }

    async fn resolve_authorization(
        &self,
        request: &ResolvedRequest,
        store: &S,
    ) -> Result<AuthorizationCheck, ServiceResponse> {
        crate::auth::resolve_authorization("sqs:UntagQueue", request, store).await
    }
}

#[cfg(test)]
mod tests {
    use chrono::{TimeZone, Utc};
    use hiraeth_core::{AuthContext, AwsAction, ResolvedRequest};
    use hiraeth_http::IncomingRequest;
    use hiraeth_store::{
        principal::Principal, sqs::SqsQueue, sqs::SqsStore, test_support::SqsTestStore,
    };

    use super::{UntagQueueAction, handle_untag_queue};

    fn resolved_request(body: &str) -> ResolvedRequest {
        ResolvedRequest {
            request: IncomingRequest {
                host: "localhost:4566".to_string(),
                method: "POST".to_string(),
                path: "/".to_string(),
                query: None,
                headers: [(
                    "x-amz-target".to_string(),
                    "AmazonSQS.UntagQueue".to_string(),
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
            <UntagQueueAction as AwsAction<SqsTestStore>>::name(&UntagQueueAction),
            "UntagQueue"
        );
    }

    #[tokio::test]
    async fn removes_requested_keys() {
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
        let request = resolved_request(
            r#"{
                "QueueUrl":"http://localhost:4566/123456789012/orders",
                "TagKeys":["owner"]
            }"#,
        );

        let response = handle_untag_queue(&request, &store)
            .await
            .expect("untag queue should succeed");

        assert_eq!(response.status_code, 200);
        assert_eq!(
            store.queue_tags(42),
            [("environment".to_string(), "test".to_string())]
                .into_iter()
                .collect()
        );
    }
}
