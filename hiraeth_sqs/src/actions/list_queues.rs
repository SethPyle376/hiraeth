use async_trait::async_trait;
use hiraeth_core::{
    AwsActionPayloadFormat, AwsActionPayloadParseError, ResolvedRequest, ServiceResponse,
    TypedAwsAction, auth::AuthorizationCheck, json_response,
};
use hiraeth_store::sqs::{SqsQueue, SqsStore};
use serde::{Deserialize, Serialize};

use super::action_support::{json_payload_format, parse_payload_error};
use crate::error::SqsError;

pub(crate) struct ListQueuesAction;

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub(crate) struct ListQueuesRequest {
    max_results: Option<i64>,
    next_token: Option<String>,
    queue_name_prefix: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "PascalCase")]
struct ListQueuesResponse {
    queue_urls: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    next_token: Option<String>,
}

async fn handle_list_queues_typed<S: SqsStore>(
    request: &ResolvedRequest,
    store: &S,
    request_body: ListQueuesRequest,
) -> Result<ServiceResponse, SqsError> {
    if let Some(max_results) = request_body.max_results
        && !(1..=1000).contains(&max_results)
    {
        return Err(SqsError::BadRequest(
            "MaxResults must be between 1 and 1000".to_string(),
        ));
    }

    let region = &request.region;
    let account_id = request.auth_context.principal.account_id.clone();
    let queue_name_prefix = request_body.queue_name_prefix.as_deref();
    let next_token = request_body.next_token.as_deref();
    let store_max_results = request_body
        .max_results
        .map(|max_results| max_results.saturating_add(1));

    let mut queues = store
        .list_queues(
            region,
            &account_id,
            queue_name_prefix,
            store_max_results,
            next_token,
        )
        .await
        .map_err(|e| SqsError::InternalError(e.to_string()))?;

    let next_token = if let Some(max_results) = request_body.max_results {
        if queues.len() as i64 > max_results {
            queues.truncate(max_results as usize);
            queues.last().map(|q| q.name.clone())
        } else {
            None
        }
    } else {
        None
    };

    let queue_urls = queues
        .into_iter()
        .map(|q| crate::util::queue_url(&request.request.host, &account_id, &q.name))
        .collect();

    json_response(&ListQueuesResponse {
        queue_urls,
        next_token,
    })
    .map_err(Into::into)
}

#[async_trait]
impl<S> TypedAwsAction<S> for ListQueuesAction
where
    S: SqsStore + Send + Sync,
{
    type Request = ListQueuesRequest;
    type Error = SqsError;

    fn name(&self) -> &'static str {
        "ListQueues"
    }

    fn payload_format(&self) -> AwsActionPayloadFormat {
        json_payload_format()
    }

    fn parse_error(&self, error: AwsActionPayloadParseError) -> SqsError {
        parse_payload_error(error)
    }

    async fn handle_typed(
        &self,
        request: ResolvedRequest,
        request_body: ListQueuesRequest,
        store: &S,
    ) -> Result<ServiceResponse, SqsError> {
        handle_list_queues_typed(&request, store, request_body).await
    }

    async fn resolve_authorization_typed(
        &self,
        request: &ResolvedRequest,
        _payload: ListQueuesRequest,
        store: &S,
    ) -> Result<AuthorizationCheck, SqsError> {
        crate::auth::resolve_authorization("sqs:ListQueues", request, store).await
    }
}

#[cfg(test)]
mod tests {
    use chrono::{TimeZone, Utc};
    use hiraeth_core::{AuthContext, ResolvedRequest, TypedAwsAction};
    use hiraeth_http::IncomingRequest;
    use hiraeth_store::{
        principal::Principal,
        sqs::SqsQueue,
        test_support::{ListQueuesCall, SqsTestStore},
    };

    use super::{ListQueuesAction, handle_list_queues_typed};
    use crate::error::SqsError;

    fn resolved_request(body: &str) -> ResolvedRequest {
        ResolvedRequest {
            request_id: "test-request-id".to_string(),
            request: IncomingRequest {
                host: "localhost:4566".to_string(),
                method: "POST".to_string(),
                path: "/".to_string(),
                query: None,
                headers: [(
                    "x-amz-target".to_string(),
                    "AmazonSQS.ListQueues".to_string(),
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
                        .with_ymd_and_hms(2026, 4, 6, 12, 0, 0)
                        .unwrap()
                        .naive_utc(),
                },
            },
            date: Utc.with_ymd_and_hms(2026, 4, 6, 12, 0, 0).unwrap(),
        }
    }

    fn queue(name: &str, region: &str, account_id: &str) -> SqsQueue {
        SqsQueue {
            id: 0,
            name: name.to_string(),
            region: region.to_string(),
            account_id: account_id.to_string(),
            queue_type: "standard".to_string(),
            visibility_timeout_seconds: 30,
            delay_seconds: 0,
            message_retention_period_seconds: 345600,
            receive_message_wait_time_seconds: 0,
            created_at: Utc
                .with_ymd_and_hms(2026, 4, 6, 12, 0, 0)
                .unwrap()
                .naive_utc(),
            updated_at: Utc
                .with_ymd_and_hms(2026, 4, 6, 12, 0, 0)
                .unwrap()
                .naive_utc(),
            ..Default::default()
        }
    }

    fn parse_json_body(response: &hiraeth_router::ServiceResponse) -> serde_json::Value {
        serde_json::from_slice(&response.body).expect("response body should be valid json")
    }

    #[test]
    fn reports_expected_action_name() {
        assert_eq!(
            <ListQueuesAction as TypedAwsAction<SqsTestStore>>::name(&ListQueuesAction),
            "ListQueues"
        );
    }

    #[tokio::test]
    async fn returns_matching_queue_urls_and_forwards_filters() {
        let store = SqsTestStore::with_queues(vec![
            queue("orders-001", "us-east-1", "123456789012"),
            queue("orders-002", "us-east-1", "123456789012"),
            queue("orders-003", "us-east-1", "123456789012"),
            queue("payments-001", "us-east-1", "123456789012"),
            queue("orders-west", "us-west-2", "123456789012"),
            queue("orders-other-account", "us-east-1", "999999999999"),
        ]);
        let request = resolved_request(
            r#"{
                "QueueNamePrefix":"orders-",
                "MaxResults":2,
                "NextToken":"orders-001"
            }"#,
        );

        let response = handle_list_queues_typed(
            &request,
            &store,
            crate::actions::test_support::parse_request_body(&request),
        )
        .await
        .expect("list queues should succeed");
        let body = parse_json_body(&response);

        assert_eq!(response.status_code, 200);
        assert_eq!(
            body["QueueUrls"],
            serde_json::json!([
                "http://localhost:4566/123456789012/orders-002",
                "http://localhost:4566/123456789012/orders-003"
            ])
        );
        assert!(body.get("NextToken").is_none());

        let calls = store.list_queues_calls();
        assert_eq!(calls.len(), 1);
        assert_eq!(
            calls[0],
            ListQueuesCall {
                region: "us-east-1".to_string(),
                account_id: "123456789012".to_string(),
                queue_name_prefix: Some("orders-".to_string()),
                max_results: Some(3),
                next_token: Some("orders-001".to_string()),
            }
        );
    }

    #[tokio::test]
    async fn returns_next_token_when_another_page_exists() {
        let store = SqsTestStore::with_queues(vec![
            queue("orders-001", "us-east-1", "123456789012"),
            queue("orders-002", "us-east-1", "123456789012"),
            queue("orders-003", "us-east-1", "123456789012"),
        ]);
        let request = resolved_request(r#"{"MaxResults":2}"#);

        let response = handle_list_queues_typed(
            &request,
            &store,
            crate::actions::test_support::parse_request_body(&request),
        )
        .await
        .expect("list queues should succeed");
        let body = parse_json_body(&response);

        assert_eq!(
            body["QueueUrls"],
            serde_json::json!([
                "http://localhost:4566/123456789012/orders-001",
                "http://localhost:4566/123456789012/orders-002"
            ])
        );
        assert_eq!(body["NextToken"], "orders-002");
    }

    #[tokio::test]
    async fn omits_next_token_when_page_is_exactly_full() {
        let store = SqsTestStore::with_queues(vec![
            queue("orders-001", "us-east-1", "123456789012"),
            queue("orders-002", "us-east-1", "123456789012"),
        ]);
        let request = resolved_request(r#"{"MaxResults":2}"#);

        let response = handle_list_queues_typed(
            &request,
            &store,
            crate::actions::test_support::parse_request_body(&request),
        )
        .await
        .expect("list queues should succeed");
        let body = parse_json_body(&response);

        assert_eq!(
            body["QueueUrls"],
            serde_json::json!([
                "http://localhost:4566/123456789012/orders-001",
                "http://localhost:4566/123456789012/orders-002"
            ])
        );
        assert!(body.get("NextToken").is_none());
    }

    #[tokio::test]
    async fn rejects_invalid_max_results() {
        let store = SqsTestStore::with_queues(Vec::new());
        let request = resolved_request(r#"{"MaxResults":0}"#);

        let result = handle_list_queues_typed(
            &request,
            &store,
            crate::actions::test_support::parse_request_body(&request),
        )
        .await;

        assert!(matches!(result, Err(SqsError::BadRequest(_))));
    }
}
