use std::{collections::HashMap, time::Instant};

use hiraeth_core::{
    ApiError, ResolvedRequest, ServiceResponse,
    tracing::{TraceContext, TraceRecorder, TraceSpanTimer},
};
use hiraeth_http::IncomingRequest;
use hiraeth_iam::{IamService, ResolveIdentityError};
use hiraeth_router::ServiceRouter;
use hiraeth_store::IamStore;

pub struct RequestTrace {
    pub auth_ms: u128,
    pub route_ms: Option<u128>,
    pub service: Option<String>,
    pub region: Option<String>,
    pub account_id: Option<String>,
    pub principal: Option<String>,
    pub access_key: Option<String>,
}

pub struct AppRequestOutcome {
    pub response: Result<ServiceResponse, ApiError>,
    pub trace: RequestTrace,
}

pub async fn resolve_and_route(
    trace_context: &TraceContext,
    trace_recorder: &impl TraceRecorder,
    incoming_request: IncomingRequest,
    iam: &IamService<impl IamStore + Send + Sync + 'static>,
    router: &ServiceRouter,
) -> AppRequestOutcome {
    let auth_started_at = Instant::now();
    let request_timer = trace_context.start_span();
    let request_trace_context = trace_context.child_context(&request_timer);
    let request_method = incoming_request.method.clone();
    let request_host = incoming_request.host.clone();
    let request_path = incoming_request.path.clone();
    let request_query = incoming_request.query.clone();
    let request_target = incoming_request.headers.get("x-amz-target").cloned();
    let request_body_bytes = incoming_request.body.len();

    let authn_timer = request_trace_context.start_span();
    let authn_trace_context = request_trace_context.child_context(&authn_timer);
    let authenticated_request = hiraeth_auth::authenticate_request(incoming_request, iam.store())
        .await
        .map_err(ApiError::from);
    record_runtime_span(
        &request_trace_context,
        trace_recorder,
        authn_timer,
        "authn.authenticate",
        if authenticated_request.is_ok() {
            "ok"
        } else {
            "error"
        },
        HashMap::new(),
    )
    .await;

    match authenticated_request {
        Ok(authenticated_request) => {
            let identity_timer = authn_trace_context.start_span();
            let identity_trace_context = authn_trace_context.child_context(&identity_timer);
            let authenticated_access_key = authenticated_request.auth_context.access_key.clone();
            let authenticated_principal_id = authenticated_request.auth_context.principal_id;
            let authenticated_service = authenticated_request.service.clone();
            let authenticated_region = authenticated_request.region.clone();
            let resolved_request = iam
                .resolve_identity(trace_context.request_id.clone(), authenticated_request)
                .await;
            record_runtime_span(
                &authn_trace_context,
                trace_recorder,
                identity_timer,
                "iam.resolve_identity",
                if resolved_request.is_ok() {
                    "ok"
                } else {
                    "error"
                },
                identity_span_attributes(
                    &resolved_request,
                    &authenticated_access_key,
                    authenticated_principal_id,
                    &authenticated_service,
                    &authenticated_region,
                ),
            )
            .await;
            let resolved_request = resolved_request.map_err(ApiError::from);
            let auth_ms = auth_started_at.elapsed().as_millis();

            match resolved_request {
                Ok(resolved_request) => {
                    let request_service = resolved_request.service.clone();
                    let request_region = resolved_request.region.clone();
                    let request_account_id =
                        resolved_request.auth_context.principal.account_id.clone();
                    let request_principal = resolved_request.auth_context.principal.name.clone();
                    let request_access_key = resolved_request.auth_context.access_key.clone();
                    let trace = RequestTrace {
                        auth_ms,
                        route_ms: None,
                        service: Some(request_service.clone()),
                        region: Some(request_region.clone()),
                        account_id: Some(request_account_id.clone()),
                        principal: Some(request_principal.clone()),
                        access_key: Some(request_access_key.clone()),
                    };

                    let route_started_at = Instant::now();
                    let response = router
                        .route_traced(resolved_request, &identity_trace_context, trace_recorder)
                        .await;
                    let route_ms = route_started_at.elapsed().as_millis();
                    let mut attributes = request_span_attributes(
                        &request_method,
                        &request_host,
                        &request_path,
                        request_query.as_deref(),
                        request_target.as_deref(),
                        request_body_bytes,
                    );
                    attributes.extend([
                        ("service".to_string(), request_service),
                        ("region".to_string(), request_region),
                        ("account_id".to_string(), request_account_id),
                        ("principal".to_string(), request_principal),
                        ("access_key".to_string(), request_access_key),
                        ("auth_ms".to_string(), auth_ms.to_string()),
                        ("route_ms".to_string(), route_ms.to_string()),
                    ]);
                    let status_code = match &response {
                        Ok(response) => response.status_code,
                        Err(error) => {
                            attributes.insert("error".to_string(), format!("{error:?}"));
                            error.status_code()
                        }
                    };
                    attributes.insert("status_code".to_string(), status_code.to_string());
                    record_runtime_span(
                        trace_context,
                        trace_recorder,
                        request_timer,
                        "request.handle",
                        if status_code < 400 { "ok" } else { "error" },
                        attributes,
                    )
                    .await;

                    AppRequestOutcome {
                        response,
                        trace: RequestTrace {
                            route_ms: Some(route_ms),
                            ..trace
                        },
                    }
                }
                Err(error) => {
                    let mut attributes = request_span_attributes(
                        &request_method,
                        &request_host,
                        &request_path,
                        request_query.as_deref(),
                        request_target.as_deref(),
                        request_body_bytes,
                    );
                    attributes.extend([
                        ("service".to_string(), authenticated_service),
                        ("region".to_string(), authenticated_region),
                        ("access_key".to_string(), authenticated_access_key),
                        (
                            "authenticated_principal_id".to_string(),
                            authenticated_principal_id.to_string(),
                        ),
                        ("auth_ms".to_string(), auth_ms.to_string()),
                        ("status_code".to_string(), error.status_code().to_string()),
                        ("error".to_string(), format!("{error:?}")),
                    ]);
                    record_runtime_span(
                        trace_context,
                        trace_recorder,
                        request_timer,
                        "request.handle",
                        "error",
                        attributes,
                    )
                    .await;

                    AppRequestOutcome {
                        response: Err(error),
                        trace: RequestTrace {
                            auth_ms,
                            route_ms: None,
                            service: None,
                            region: None,
                            account_id: None,
                            principal: None,
                            access_key: None,
                        },
                    }
                }
            }
        }
        Err(error) => {
            let auth_ms = auth_started_at.elapsed().as_millis();
            let mut attributes = request_span_attributes(
                &request_method,
                &request_host,
                &request_path,
                request_query.as_deref(),
                request_target.as_deref(),
                request_body_bytes,
            );
            attributes.extend([
                ("auth_ms".to_string(), auth_ms.to_string()),
                ("status_code".to_string(), error.status_code().to_string()),
                ("error".to_string(), format!("{error:?}")),
            ]);
            record_runtime_span(
                trace_context,
                trace_recorder,
                request_timer,
                "request.handle",
                "error",
                attributes,
            )
            .await;

            AppRequestOutcome {
                response: Err(error),
                trace: RequestTrace {
                    auth_ms,
                    route_ms: None,
                    service: None,
                    region: None,
                    account_id: None,
                    principal: None,
                    access_key: None,
                },
            }
        }
    }
}

fn request_span_attributes(
    method: &str,
    host: &str,
    path: &str,
    query: Option<&str>,
    target: Option<&str>,
    body_bytes: usize,
) -> HashMap<String, String> {
    let mut attributes = HashMap::from([
        ("method".to_string(), method.to_string()),
        ("host".to_string(), host.to_string()),
        ("path".to_string(), path.to_string()),
        ("request_bytes".to_string(), body_bytes.to_string()),
    ]);

    if let Some(query) = query {
        attributes.insert("query".to_string(), query.to_string());
    }

    if let Some(target) = target {
        attributes.insert("target".to_string(), target.to_string());
    }

    attributes
}

async fn record_runtime_span(
    trace_context: &TraceContext,
    trace_recorder: &impl TraceRecorder,
    timer: TraceSpanTimer,
    name: &'static str,
    status: &'static str,
    attributes: HashMap<String, String>,
) {
    if let Err(error) = trace_context
        .record_span(trace_recorder, timer, name, "runtime", status, attributes)
        .await
    {
        tracing::warn!(error = ?error, span = name, "failed to record trace span");
    }
}

fn identity_span_attributes(
    resolved_request: &Result<ResolvedRequest, ResolveIdentityError>,
    authenticated_access_key: &str,
    authenticated_principal_id: i64,
    authenticated_service: &str,
    authenticated_region: &str,
) -> HashMap<String, String> {
    let mut attributes = HashMap::from([
        (
            "access_key".to_string(),
            authenticated_access_key.to_string(),
        ),
        (
            "authenticated_principal_id".to_string(),
            authenticated_principal_id.to_string(),
        ),
        ("service".to_string(), authenticated_service.to_string()),
        ("region".to_string(), authenticated_region.to_string()),
    ]);

    match resolved_request {
        Ok(request) => {
            let principal = &request.auth_context.principal;
            attributes.extend([
                ("principal_id".to_string(), principal.id.to_string()),
                (
                    "principal_account_id".to_string(),
                    principal.account_id.clone(),
                ),
                ("principal_kind".to_string(), principal.kind.clone()),
                ("principal_name".to_string(), principal.name.clone()),
                ("principal_path".to_string(), principal.path.clone()),
                ("principal_user_id".to_string(), principal.user_id.clone()),
            ]);
        }
        Err(error) => {
            attributes.insert("error".to_string(), format!("{error:?}"));
        }
    }

    attributes
}

#[cfg(test)]
mod tests {
    use chrono::{TimeZone, Utc};
    use hiraeth_core::{AuthContext, ResolvedRequest};
    use hiraeth_http::IncomingRequest;
    use hiraeth_iam::ResolveIdentityError;
    use hiraeth_store::iam::Principal;

    use super::identity_span_attributes;

    #[test]
    fn identity_span_attributes_include_resolved_principal_details() {
        let resolved_request = Ok(ResolvedRequest {
            request_id: "request-id".to_string(),
            request: IncomingRequest {
                host: "sqs.us-east-1.amazonaws.com".to_string(),
                method: "POST".to_string(),
                path: "/".to_string(),
                query: None,
                headers: Default::default(),
                body: Vec::new(),
            },
            service: "sqs".to_string(),
            region: "us-east-1".to_string(),
            auth_context: AuthContext {
                access_key: "AKIAIOSFODNN7EXAMPLE".to_string(),
                principal: Principal {
                    id: 42,
                    account_id: "123456789012".to_string(),
                    kind: "user".to_string(),
                    name: "alice".to_string(),
                    path: "/engineering/".to_string(),
                    user_id: "AIDATESTUSER000042".to_string(),
                    created_at: Utc
                        .with_ymd_and_hms(2026, 4, 28, 12, 0, 0)
                        .unwrap()
                        .naive_utc(),
                },
            },
            date: Utc.with_ymd_and_hms(2026, 4, 28, 12, 0, 0).unwrap(),
        });

        let attributes = identity_span_attributes(
            &resolved_request,
            "AKIAIOSFODNN7EXAMPLE",
            42,
            "sqs",
            "us-east-1",
        );

        assert_eq!(
            attributes.get("access_key").map(String::as_str),
            Some("AKIAIOSFODNN7EXAMPLE")
        );
        assert_eq!(
            attributes
                .get("authenticated_principal_id")
                .map(String::as_str),
            Some("42")
        );
        assert_eq!(
            attributes.get("principal_id").map(String::as_str),
            Some("42")
        );
        assert_eq!(
            attributes.get("principal_account_id").map(String::as_str),
            Some("123456789012")
        );
        assert_eq!(
            attributes.get("principal_kind").map(String::as_str),
            Some("user")
        );
        assert_eq!(
            attributes.get("principal_name").map(String::as_str),
            Some("alice")
        );
        assert_eq!(
            attributes.get("principal_path").map(String::as_str),
            Some("/engineering/")
        );
        assert_eq!(
            attributes.get("principal_user_id").map(String::as_str),
            Some("AIDATESTUSER000042")
        );
    }

    #[test]
    fn identity_span_attributes_include_error_when_identity_resolution_fails() {
        let resolved_request = Err(ResolveIdentityError::PrincipalNotFound);

        let attributes = identity_span_attributes(
            &resolved_request,
            "AKIAIOSFODNN7EXAMPLE",
            42,
            "sqs",
            "us-east-1",
        );

        assert_eq!(
            attributes.get("access_key").map(String::as_str),
            Some("AKIAIOSFODNN7EXAMPLE")
        );
        assert_eq!(
            attributes
                .get("authenticated_principal_id")
                .map(String::as_str),
            Some("42")
        );
        assert_eq!(
            attributes.get("error").map(String::as_str),
            Some("PrincipalNotFound")
        );
        assert!(!attributes.contains_key("principal_name"));
    }
}
