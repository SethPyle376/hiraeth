use std::{collections::HashMap, time::Instant};

use hiraeth_core::{
    ApiError, ResolvedRequest, ServiceResponse,
    tracing::{TraceContext, TraceRecorder},
};
use hiraeth_http::IncomingRequest;
use hiraeth_iam::IamService;
use hiraeth_router::ServiceRouter;

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
    trace_recorder: &(impl TraceRecorder),
    incoming_request: IncomingRequest,
    iam: &IamService<impl hiraeth_store::IamStore + Send + Sync + 'static>,
    router: &ServiceRouter,
) -> AppRequestOutcome {
    let auth_started_at = Instant::now();
    let authn_timer = trace_context.start_span();
    let authenticated_request = hiraeth_auth::authenticate_request(incoming_request, iam.store())
        .await
        .map_err(ApiError::from);
    record_runtime_span(
        trace_context,
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
            let identity_timer = trace_context.start_span();
            let authenticated_access_key = authenticated_request.auth_context.access_key.clone();
            let authenticated_principal_id = authenticated_request.auth_context.principal_id;
            let authenticated_service = authenticated_request.service.clone();
            let authenticated_region = authenticated_request.region.clone();
            let resolved_request = iam
                .resolve_identity(trace_context.request_id.clone(), authenticated_request)
                .await;
            record_runtime_span(
                trace_context,
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
                    let trace = RequestTrace {
                        auth_ms,
                        route_ms: None,
                        service: Some(resolved_request.service.clone()),
                        region: Some(resolved_request.region.clone()),
                        account_id: Some(
                            resolved_request.auth_context.principal.account_id.clone(),
                        ),
                        principal: Some(resolved_request.auth_context.principal.name.clone()),
                        access_key: Some(resolved_request.auth_context.access_key.clone()),
                    };

                    let route_started_at = Instant::now();
                    let response = router
                        .route_traced(resolved_request, trace_context, trace_recorder)
                        .await;
                    let route_ms = route_started_at.elapsed().as_millis();

                    AppRequestOutcome {
                        response,
                        trace: RequestTrace {
                            route_ms: Some(route_ms),
                            ..trace
                        },
                    }
                }
                Err(error) => AppRequestOutcome {
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
                },
            }
        }
        Err(error) => {
            let auth_ms = auth_started_at.elapsed().as_millis();

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

async fn record_runtime_span(
    trace_context: &TraceContext,
    trace_recorder: &(impl TraceRecorder),
    timer: hiraeth_core::tracing::TraceSpanTimer,
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
    resolved_request: &Result<ResolvedRequest, hiraeth_iam::ResolveIdentityError>,
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
