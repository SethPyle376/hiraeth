use async_trait::async_trait;
use hiraeth_core::{
    ApiError, AuthContext, AwsActionRegistry, ResolvedRequest, ServiceResponse,
    auth::AuthorizationCheck,
};
use hiraeth_router::Service;
use hiraeth_store::sqs::SqsStore;

mod actions;
mod auth;
mod error;
mod util;

pub struct SqsService<S: SqsStore> {
    store: S,
    actions: AwsActionRegistry<S>,
}

impl<S> SqsService<S>
where
    S: SqsStore + Send + Sync + 'static,
{
    pub fn new(store: S) -> Self {
        Self {
            store,
            actions: actions::registry(),
        }
    }
}

#[async_trait]
impl<S> Service for SqsService<S>
where
    S: SqsStore + Send + Sync + 'static,
{
    fn can_handle(&self, request: &ResolvedRequest) -> bool {
        request.service == "sqs"
    }

    async fn handle_request(
        &self,
        request: ResolvedRequest,
    ) -> Result<ServiceResponse, hiraeth_core::ApiError> {
        let action_name = match auth::get_action_name_for_request(&request) {
            Ok(action_name) => action_name,
            Err(error::SqsError::BadRequest(message))
                if message == "Missing x-amz-target header" =>
            {
                return Err(ApiError::NotFound(message));
            }
            Err(error) => return Ok(ServiceResponse::from(error)),
        };
        let action = match self.actions.get(&action_name) {
            Some(action) => action,
            None => {
                return Ok(ServiceResponse::from(
                    error::SqsError::UnsupportedOperation(action_name),
                ));
            }
        };

        Ok(action.handle(request, &self.store).await)
    }

    async fn resolve_authorization(
        &self,
        request: &ResolvedRequest,
    ) -> Result<AuthorizationCheck, ServiceResponse> {
        let action_name =
            auth::get_action_name_for_request(request).map_err(ServiceResponse::from)?;
        let action = self.actions.get(&action_name).ok_or_else(|| {
            ServiceResponse::from(error::SqsError::UnsupportedOperation(action_name.clone()))
        })?;

        action.resolve_authorization(request, &self.store).await
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::{Service, ServiceResponse, SqsService};
    use chrono::{TimeZone, Utc};
    use hiraeth_core::{AuthContext, ResolvedRequest};
    use hiraeth_http::IncomingRequest;
    use hiraeth_store::{principal::Principal, sqs::SqsQueue, test_support::SqsTestStore};
    use serde_json::Value;

    fn resolved_request(target: Option<&str>, body: &str) -> ResolvedRequest {
        let mut headers = HashMap::new();
        if let Some(target) = target {
            headers.insert("x-amz-target".to_string(), target.to_string());
        }

        ResolvedRequest {
            request_id: "test-request-id".to_string(),
            request: IncomingRequest {
                host: "sqs.us-east-1.amazonaws.com".to_string(),
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

    fn parse_json_body(response: &ServiceResponse) -> Value {
        serde_json::from_slice(&response.body).expect("response body should be valid json")
    }

    #[tokio::test]
    async fn resolve_authorization_returns_action_and_resource_for_queue_action() {
        let service = SqsService::new(SqsTestStore::with_queue(SqsQueue {
            id: 1,
            name: "existing-queue".to_string(),
            region: "us-east-1".to_string(),
            account_id: "123456789012".to_string(),
            ..Default::default()
        }));
        let request = resolved_request(
            Some("AmazonSQS.SendMessage"),
            r#"{
                "QueueUrl":"http://sqs.us-east-1.amazonaws.com/123456789012/existing-queue",
                "MessageBody":"hello"
            }"#,
        );

        let check = service
            .resolve_authorization(&request)
            .await
            .expect("auth check should resolve queue context");

        assert_eq!(check.action, "sqs:SendMessage");
        assert_eq!(
            check.resource,
            "arn:aws:sqs:us-east-1:123456789012:existing-queue"
        );
        assert!(check.resource_policy.is_some());
    }

    #[tokio::test]
    async fn resolve_authorization_returns_account_scoped_resource_for_create_queue() {
        let service = SqsService::new(SqsTestStore::default());
        let request = resolved_request(
            Some("AmazonSQS.CreateQueue"),
            r#"{"QueueName":"new-queue"}"#,
        );

        let check = service
            .resolve_authorization(&request)
            .await
            .expect("auth check should resolve create queue context");

        assert_eq!(check.action, "sqs:CreateQueue");
        assert_eq!(check.resource, "arn:aws:sqs:us-east-1:123456789012:*");
        assert!(check.resource_policy.is_none());
    }

    #[tokio::test]
    async fn resolve_authorization_renders_sqs_error_response_for_queue_lookup_failure() {
        let service = SqsService::new(SqsTestStore::default());
        let request = resolved_request(
            Some("AmazonSQS.SendMessage"),
            r#"{
                "QueueUrl":"http://sqs.us-east-1.amazonaws.com/123456789012/missing-queue",
                "MessageBody":"hello"
            }"#,
        );

        let response = service
            .resolve_authorization(&request)
            .await
            .expect_err("queue lookup failures should render as SQS responses");

        assert_eq!(response.status_code, 400);
        assert_eq!(
            response
                .headers
                .iter()
                .find(|(name, _)| name == "x-amzn-query-error")
                .map(|(_, value)| value.as_str()),
            Some("AWS.SimpleQueueService.NonExistentQueue;Sender")
        );

        let body = parse_json_body(&response);
        assert_eq!(body["__type"], "com.amazonaws.sqs#QueueDoesNotExist");
        assert_eq!(body["message"], "The specified queue does not exist.");
    }

    #[tokio::test]
    async fn service_returns_not_found_for_missing_target_header() {
        let service = SqsService::new(SqsTestStore::default());
        let request = resolved_request(None, r#"{"QueueName":"test-queue"}"#);

        let result = service.handle_request(request).await;

        assert!(matches!(
            result,
            Err(hiraeth_core::ApiError::NotFound(message))
                if message == "Missing x-amz-target header"
        ));
    }

    #[tokio::test]
    async fn service_returns_not_found_for_unknown_action() {
        let service = SqsService::new(SqsTestStore::default());
        let request = resolved_request(Some("AmazonSQS.DoesNotExist"), "{}");

        let response = service
            .handle_request(request)
            .await
            .expect("unknown SQS action should render an SQS error response");

        assert_eq!(response.status_code, 400);
        assert_eq!(
            response
                .headers
                .iter()
                .find(|(name, _)| name == "x-amzn-query-error")
                .map(|(_, value)| value.as_str()),
            Some("AWS.SimpleQueueService.UnsupportedOperation;Sender")
        );

        let body = parse_json_body(&response);
        assert_eq!(body["__type"], "com.amazonaws.sqs#UnsupportedOperation");
        assert_eq!(body["message"], "AmazonSQS.DoesNotExist");
    }

    #[tokio::test]
    async fn service_renders_queue_not_found_as_sqs_error_response() {
        let service = SqsService::new(SqsTestStore::default());
        let request = resolved_request(
            Some("AmazonSQS.GetQueueUrl"),
            r#"{"QueueName":"missing-queue"}"#,
        );

        let response = service
            .handle_request(request)
            .await
            .expect("service should render SQS errors as a response");

        assert_eq!(response.status_code, 400);
        assert_eq!(
            response
                .headers
                .iter()
                .find(|(name, _)| name == "content-type")
                .map(|(_, value)| value.as_str()),
            Some("application/x-amz-json-1.0")
        );
        assert_eq!(
            response
                .headers
                .iter()
                .find(|(name, _)| name == "x-amzn-query-error")
                .map(|(_, value)| value.as_str()),
            Some("AWS.SimpleQueueService.NonExistentQueue;Sender")
        );

        let body = parse_json_body(&response);
        assert_eq!(body["__type"], "com.amazonaws.sqs#QueueDoesNotExist");
        assert_eq!(body["message"], "The specified queue does not exist.");
    }
}
