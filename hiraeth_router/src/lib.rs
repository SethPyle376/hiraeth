mod authorizer;
mod service;

pub use authorizer::{AuthorizationOutcome, AuthorizationResult, Authorizer};
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
        let route_timer = trace_context.start_span();
        let route_trace_context = trace_context.child_context(&route_timer);

        let resolve_service_timer = route_trace_context.start_span();
        let resolve_service_trace_context =
            route_trace_context.child_context(&resolve_service_timer);
        let service = match self.services.iter().find(|s| s.can_handle(&request)) {
            Some(service) => {
                record_router_span(
                    &route_trace_context,
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
                service
            }
            None => {
                record_router_span(
                    &route_trace_context,
                    trace_recorder,
                    resolve_service_timer,
                    "router.resolve_service",
                    "error",
                    [
                        ("service".to_string(), request.service.clone()),
                        ("region".to_string(), request.region.clone()),
                        (
                            "error".to_string(),
                            "No service found to handle the request".to_string(),
                        ),
                    ],
                )
                .await;
                record_router_span(
                    trace_context,
                    trace_recorder,
                    route_timer,
                    "router.route",
                    "error",
                    route_span_attributes(&request, None, None),
                )
                .await;
                return Err(ApiError::from(ServiceRouterError::NoServiceFound));
            }
        };

        let resolve_check_timer = resolve_service_trace_context.start_span();
        let resolve_check_trace_context =
            resolve_service_trace_context.child_context(&resolve_check_timer);
        let check = match service.resolve_authorization(&request).await {
            Ok(check) => {
                record_router_span(
                    &resolve_service_trace_context,
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
                    &resolve_service_trace_context,
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
                record_router_span(
                    trace_context,
                    trace_recorder,
                    route_timer,
                    "router.route",
                    "error",
                    route_span_attributes(&request, Some(response.status_code), None),
                )
                .await;
                return Ok(response);
            }
        };

        let auth_result = self
            .authorizer
            .authorize(
                &request,
                &check,
                &resolve_check_trace_context,
                trace_recorder,
            )
            .await;

        match auth_result.result {
            AuthorizationResult::Allow => {
                let service_timer = auth_result.trace_context.start_span();
                let service_trace_context = auth_result.trace_context.child_context(&service_timer);
                let mut route_attributes =
                    route_span_attributes(&request, None, Some(&check.action));
                let result = service
                    .handle_request(request, &service_trace_context, trace_recorder)
                    .await;
                let status_code = result.as_ref().ok().map(|response| response.status_code);
                if let Some(status_code) = status_code {
                    route_attributes.insert("status_code".to_string(), status_code.to_string());
                }
                let route_status = if result.is_ok() { "ok" } else { "error" };
                record_router_span(
                    &route_trace_context,
                    trace_recorder,
                    service_timer,
                    "service.handle",
                    route_status,
                    [("action".to_string(), check.action.clone())],
                )
                .await;
                record_router_span(
                    trace_context,
                    trace_recorder,
                    route_timer,
                    "router.route",
                    route_status,
                    route_attributes,
                )
                .await;
                result
            }
            AuthorizationResult::Deny => {
                let response = self.authorizer.unauthorized_response();
                record_router_span(
                    trace_context,
                    trace_recorder,
                    route_timer,
                    "router.route",
                    "deny",
                    route_span_attributes(
                        &request,
                        Some(response.status_code),
                        Some(&check.action),
                    ),
                )
                .await;
                Ok(response)
            }
        }
    }

    pub fn register_service(&mut self, service: Box<dyn Service + Send + Sync>) {
        self.services.push(service);
    }
}

fn route_span_attributes(
    request: &ResolvedRequest,
    status_code: Option<u16>,
    action: Option<&str>,
) -> HashMap<String, String> {
    let mut attributes = HashMap::from([
        ("service".to_string(), request.service.clone()),
        ("region".to_string(), request.region.clone()),
        (
            "account_id".to_string(),
            request.auth_context.principal.account_id.clone(),
        ),
        (
            "principal".to_string(),
            request.auth_context.principal.name.clone(),
        ),
    ]);

    if let Some(status_code) = status_code {
        attributes.insert("status_code".to_string(), status_code.to_string());
    }

    if let Some(action) = action {
        attributes.insert("action".to_string(), action.to_string());
    }

    attributes
}

async fn record_router_span<R, I>(
    trace_context: &TraceContext,
    trace_recorder: &R,
    timer: hiraeth_core::tracing::TraceSpanTimer,
    name: &'static str,
    status: &'static str,
    attributes: I,
) where
    R: TraceRecorder,
    I: IntoIterator<Item = (String, String)>,
{
    if let Err(error) = trace_context
        .record_span(
            trace_recorder,
            timer,
            name,
            "router",
            status,
            HashMap::from_iter(attributes),
        )
        .await
    {
        tracing::warn!(error = ?error, span = name, "failed to record trace span");
    }
}

#[cfg(test)]
mod tests {
    use std::{collections::HashMap, sync::Mutex};

    use async_trait::async_trait;
    use chrono::{TimeZone, Utc};
    use hiraeth_core::{
        ApiError, AuthContext, ResolvedRequest, ServiceResponse,
        auth::AuthorizationCheck,
        tracing::{
            CompletedRequestTrace, TraceContext, TraceRecordError, TraceRecorder, TraceSpanRecord,
        },
    };
    use hiraeth_http::IncomingRequest;
    use hiraeth_store::principal::Principal;

    use super::{AuthorizationOutcome, AuthorizationResult, Authorizer, Service, ServiceRouter};

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
            trace_context: &TraceContext,
            trace_recorder: &dyn TraceRecorder,
        ) -> AuthorizationOutcome {
            let timer = trace_context.start_span();
            let authz_trace_context = trace_context.child_context(&timer);
            trace_context
                .record_span(
                    trace_recorder,
                    timer,
                    "authz.evaluate",
                    "iam",
                    self.result.as_trace_status(),
                    HashMap::new(),
                )
                .await
                .expect("authz span should record");
            AuthorizationOutcome::new(self.result, authz_trace_context)
        }

        fn unauthorized_response(&self) -> ServiceResponse {
            ServiceResponse {
                status_code: 403,
                body: Vec::new(),
                headers: Vec::new(),
            }
        }
    }

    #[derive(Default)]
    struct RecordingTraceRecorder {
        spans: Mutex<Vec<TraceSpanRecord>>,
    }

    #[async_trait]
    impl TraceRecorder for RecordingTraceRecorder {
        async fn record_request_trace(
            &self,
            _trace: CompletedRequestTrace,
        ) -> Result<(), TraceRecordError> {
            unreachable!("router tests only record spans")
        }

        async fn record_span(&self, span: TraceSpanRecord) -> Result<(), TraceRecordError> {
            self.spans
                .lock()
                .expect("trace recorder mutex should not be poisoned")
                .push(span);
            Ok(())
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

    #[tokio::test]
    async fn route_traced_records_router_spans_as_sequential_chain() {
        let mut router = ServiceRouter::new(Box::new(TestAuthorizer {
            result: AuthorizationResult::Allow,
        }));
        router.register_service(Box::new(AuthorizedService));
        let trace_recorder = RecordingTraceRecorder::default();
        let trace_context = TraceContext::new("trace-request-id");

        let response = router
            .route_traced(resolved_request(), &trace_context, &trace_recorder)
            .await
            .expect("router should execute authorized service");

        assert_eq!(response.status_code, 200);

        let spans = trace_recorder
            .spans
            .lock()
            .expect("trace recorder mutex should not be poisoned");
        let route_span = spans
            .iter()
            .find(|span| span.name == "router.route")
            .expect("route span should be recorded");

        assert!(route_span.parent_span_id.is_none());

        for (child_name, parent_name) in [
            ("router.resolve_service", "router.route"),
            ("authz.resolve_check", "router.resolve_service"),
            ("authz.evaluate", "authz.resolve_check"),
            ("service.handle", "authz.evaluate"),
        ] {
            let child_span = spans
                .iter()
                .find(|span| span.name == child_name)
                .unwrap_or_else(|| panic!("{child_name} span should be recorded"));
            let parent_span = spans
                .iter()
                .find(|span| span.name == parent_name)
                .unwrap_or_else(|| panic!("{parent_name} span should be recorded"));
            assert_eq!(
                child_span.parent_span_id.as_deref(),
                Some(parent_span.span_id.as_str())
            );
        }
    }
}
