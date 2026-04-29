mod authorizer;
mod service;

pub use authorizer::{AuthorizationResult, Authorizer};
pub use hiraeth_core::ServiceResponse;
use hiraeth_core::{
    ApiError, ResolvedRequest,
    tracing::{NoopTraceRecorder, TraceContext, TraceRecorder},
};
pub use service::Service;

use std::collections::HashMap;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ServiceRouterError {
    NoServiceFound,
}

impl From<ServiceRouterError> for ApiError {
    fn from(value: ServiceRouterError) -> ApiError {
        match value {
            ServiceRouterError::NoServiceFound => {
                ApiError::NotFound("No service found to handle the request".to_string())
            }
        }
    }
}

pub struct ServiceRouter {
    authorizer: Box<dyn Authorizer + Send + Sync>,
    services: Vec<Box<dyn Service + Send + Sync>>,
}

impl ServiceRouter {
    pub fn new(authorizer: Box<dyn Authorizer + Send + Sync>) -> Self {
        Self {
            authorizer,
            services: Vec::new(),
        }
    }
}

impl ServiceRouter {
    pub async fn route(&self, request: ResolvedRequest) -> Result<ServiceResponse, ApiError> {
        self.route_traced(request, &TraceContext::new("noop"), &NoopTraceRecorder)
            .await
    }

    pub async fn route_traced<R>(
        &self,
        request: ResolvedRequest,
        trace_context: &TraceContext,
        trace_recorder: &R,
    ) -> Result<ServiceResponse, ApiError>
    where
        R: TraceRecorder,
    {
        let resolve_service_timer = trace_context.start_span();
        let service = self
            .services
            .iter()
            .find(|s| s.can_handle(&request))
            .ok_or(ApiError::from(ServiceRouterError::NoServiceFound))?;
        record_router_span(
            trace_context,
            trace_recorder,
            resolve_service_timer,
            "router.resolve_service",
            "ok",
            [
                ("service".to_string(), request.service.clone()),
                ("region".to_string(), request.region.clone()),
            ],
        )
        .await;

        let resolve_check_timer = trace_context.start_span();
        let check = match service.resolve_authorization(&request).await {
            Ok(check) => {
                record_router_span(
                    trace_context,
                    trace_recorder,
                    resolve_check_timer,
                    "authz.resolve_check",
                    "ok",
                    [
                        ("action".to_string(), check.action.clone()),
                        ("resource".to_string(), check.resource.clone()),
                    ],
                )
                .await;
                check
            }
            Err(response) => {
                record_router_span(
                    trace_context,
                    trace_recorder,
                    resolve_check_timer,
                    "authz.resolve_check",
                    "error",
                    [
                        ("status_code".to_string(), response.status_code.to_string()),
                        (
                            "response".to_string(),
                            String::from_utf8_lossy(&response.body).to_string(),
                        ),
                    ],
                )
                .await;
                return Ok(response);
            }
        };

        let auth_result = self
            .authorizer
            .authorize(&request, &check, trace_context, trace_recorder)
            .await;

        match auth_result {
            AuthorizationResult::Allow => {
                let service_timer = trace_context.start_span();
                let result = service
                    .handle_request(request, trace_context, trace_recorder)
                    .await;
                record_router_span(
                    trace_context,
                    trace_recorder,
                    service_timer,
                    "service.handle",
                    if result.is_ok() { "ok" } else { "error" },
                    [("action".to_string(), check.action)],
                )
                .await;
                result
            }
            AuthorizationResult::Deny => {
                let response = self.authorizer.unauthorized_response();
                Ok(response)
            }
        }
    }

    pub fn register_service(&mut self, service: Box<dyn Service + Send + Sync>) {
        self.services.push(service);
    }
}

async fn record_router_span<R, const N: usize>(
    trace_context: &TraceContext,
    trace_recorder: &R,
    timer: hiraeth_core::tracing::TraceSpanTimer,
    name: &'static str,
    status: &'static str,
    attributes: [(String, String); N],
) where
    R: TraceRecorder,
{
    if let Err(error) = trace_context
        .record_span(
            trace_recorder,
            timer,
            name,
            "router",
            status,
            HashMap::from(attributes),
        )
        .await
    {
        tracing::warn!(error = ?error, span = name, "failed to record trace span");
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use async_trait::async_trait;
    use chrono::{TimeZone, Utc};
    use hiraeth_core::{
        ApiError, AuthContext, ResolvedRequest, ServiceResponse,
        auth::AuthorizationCheck,
        tracing::{TraceContext, TraceRecorder},
    };
    use hiraeth_http::IncomingRequest;
    use hiraeth_store::principal::Principal;

    use super::{AuthorizationResult, Authorizer, Service, ServiceRouter};

    fn resolved_request() -> ResolvedRequest {
        ResolvedRequest {
            request_id: "test-request-id".to_string(),
            request: IncomingRequest {
                host: "sqs.us-east-1.amazonaws.com".to_string(),
                method: "POST".to_string(),
                path: "/".to_string(),
                query: None,
                headers: HashMap::new(),
                body: Vec::new(),
            },
            service: "test".to_string(),
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
                        .with_ymd_and_hms(2026, 4, 20, 12, 0, 0)
                        .unwrap()
                        .naive_utc(),
                },
            },
            date: Utc.with_ymd_and_hms(2026, 4, 20, 12, 0, 0).unwrap(),
        }
    }

    struct AuthorizedService;

    #[async_trait]
    impl Service for AuthorizedService {
        fn can_handle(&self, request: &ResolvedRequest) -> bool {
            request.service == "test"
        }

        async fn handle_request(
            &self,
            _request: ResolvedRequest,
            _trace_context: &TraceContext,
            _trace_recorder: &dyn TraceRecorder,
        ) -> Result<ServiceResponse, ApiError> {
            Ok(ServiceResponse {
                status_code: 200,
                body: Vec::new(),
                headers: Vec::new(),
            })
        }

        async fn resolve_authorization(
            &self,
            _request: &ResolvedRequest,
        ) -> Result<AuthorizationCheck, ServiceResponse> {
            Ok(AuthorizationCheck {
                action: "test:Action".to_string(),
                resource: "*".to_string(),
                resource_policy: None,
            })
        }
    }

    struct AuthorizationErrorService;

    #[async_trait]
    impl Service for AuthorizationErrorService {
        fn can_handle(&self, request: &ResolvedRequest) -> bool {
            request.service == "test"
        }

        async fn handle_request(
            &self,
            _request: ResolvedRequest,
            _trace_context: &TraceContext,
            _trace_recorder: &dyn TraceRecorder,
        ) -> Result<ServiceResponse, ApiError> {
            panic!("service should not execute when authorization resolution fails");
        }

        async fn resolve_authorization(
            &self,
            _request: &ResolvedRequest,
        ) -> Result<AuthorizationCheck, ServiceResponse> {
            Err(ServiceResponse {
                status_code: 418,
                body: Vec::new(),
                headers: Vec::new(),
            })
        }
    }

    struct TestAuthorizer {
        result: AuthorizationResult,
    }

    #[async_trait]
    impl Authorizer for TestAuthorizer {
        async fn authorize(
            &self,
            _request: &ResolvedRequest,
            _check: &AuthorizationCheck,
            _trace_context: &TraceContext,
            _trace_recorder: &dyn TraceRecorder,
        ) -> AuthorizationResult {
            self.result
        }

        fn unauthorized_response(&self) -> ServiceResponse {
            ServiceResponse {
                status_code: 403,
                body: Vec::new(),
                headers: Vec::new(),
            }
        }
    }

    #[tokio::test]
    async fn route_returns_service_response_when_authorization_resolution_fails() {
        let mut router = ServiceRouter::new(Box::new(TestAuthorizer {
            result: AuthorizationResult::Deny,
        }));
        router.register_service(Box::new(AuthorizationErrorService));

        let response = router
            .route(resolved_request())
            .await
            .expect("router should return service authorization response");

        assert_eq!(response.status_code, 418);
    }

    #[tokio::test]
    async fn route_returns_unauthorized_response_when_authorization_denies() {
        let mut router = ServiceRouter::new(Box::new(TestAuthorizer {
            result: AuthorizationResult::Deny,
        }));
        router.register_service(Box::new(AuthorizedService));

        let response = router
            .route(resolved_request())
            .await
            .expect("router should return unauthorized response");

        assert_eq!(response.status_code, 403);
    }

    #[tokio::test]
    async fn route_executes_service_when_authorization_allows() {
        let mut router = ServiceRouter::new(Box::new(TestAuthorizer {
            result: AuthorizationResult::Allow,
        }));
        router.register_service(Box::new(AuthorizedService));

        let response = router
            .route(resolved_request())
            .await
            .expect("router should execute authorized service");

        assert_eq!(response.status_code, 200);
    }
}
