use async_trait::async_trait;
use hiraeth_core::{
    ApiError, AwsAction, ResolvedRequest, ServiceResponse, auth::AuthorizationCheck, empty_response,
};
use hiraeth_store::sqs::{SqsQueue, SqsStore};
use serde::Deserialize;

use crate::error::SqsError;

pub(crate) struct PurgeQueueAction;

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct PurgeQueueRequest {
    queue_url: String,
}

async fn handle_purge_queue<S: SqsStore>(
    request: &ResolvedRequest,
    store: &S,
) -> Result<ServiceResponse, SqsError> {
    let request_body = crate::util::parse_request_body::<PurgeQueueRequest>(request)?;
    let queue = crate::util::load_queue_from_url(request, store, &request_body.queue_url).await?;

    store
        .purge_queue(queue.id)
        .await
        .map(|_| empty_response())
        .map_err(crate::error::map_store_error)
}

#[async_trait]
impl<S> AwsAction<S> for PurgeQueueAction
where
    S: SqsStore + Send + Sync,
{
    fn name(&self) -> &'static str {
        "PurgeQueue"
    }

    async fn handle(
        &self,
        request: ResolvedRequest,
        store: &S,
    ) -> Result<ServiceResponse, ApiError> {
        match handle_purge_queue(&request, store).await {
            Ok(response) => Ok(response),
            Err(error) => Ok(ServiceResponse::from(error)),
        }
    }

    async fn resolve_authorization(
        &self,
        request: &ResolvedRequest,
        store: &S,
    ) -> Result<AuthorizationCheck, ServiceResponse> {
        crate::auth::resolve_authorization("sqs:PurgeQueue", request, store).await
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use chrono::{TimeZone, Utc};
    use hiraeth_core::{AuthContext, AwsAction, ResolvedRequest};
    use hiraeth_http::IncomingRequest;
    use hiraeth_store::{principal::Principal, sqs::SqsQueue, test_support::SqsTestStore};

    use super::{PurgeQueueAction, handle_purge_queue};
    use crate::error::SqsError;

    fn resolved_request(body: &str) -> ResolvedRequest {
        let mut headers = HashMap::new();
        headers.insert(
            "x-amz-target".to_string(),
            "AmazonSQS.PurgeQueue".to_string(),
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
            <PurgeQueueAction as AwsAction<SqsTestStore>>::name(&PurgeQueueAction),
            "PurgeQueue"
        );
    }

    #[tokio::test]
    async fn purges_existing_queue() {
        let store = SqsTestStore::with_queue(queue());
        let request =
            resolved_request(r#"{"QueueUrl":"http://localhost:4566/123456789012/orders"}"#);

        let response = handle_purge_queue(&request, &store)
            .await
            .expect("purge queue should succeed");

        assert_eq!(response.status_code, 200);
        assert!(response.body.is_empty());
        assert_eq!(store.purged_queue_ids(), vec![42]);
    }

    #[tokio::test]
    async fn returns_not_found_for_missing_queue() {
        let store = SqsTestStore::default();
        let request =
            resolved_request(r#"{"QueueUrl":"http://localhost:4566/123456789012/orders"}"#);

        let result = handle_purge_queue(&request, &store).await;

        assert!(matches!(result, Err(SqsError::QueueNotFound)));
        assert!(store.purged_queue_ids().is_empty());
    }
}
