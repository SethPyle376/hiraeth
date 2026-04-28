use std::time::Instant;

use hiraeth_core::{
    ApiError, ServiceResponse,
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
    trace_recorder: &(impl TraceRecorder + Sync),
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
    )
    .await;

    match authenticated_request {
        Ok(authenticated_request) => {
            let identity_timer = trace_context.start_span();
            let resolved_request = iam
                .resolve_identity(trace_context.request_id.clone(), authenticated_request)
                .await
                .map_err(ApiError::from);
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
            )
            .await;
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
    trace_recorder: &(impl TraceRecorder + Sync),
    timer: hiraeth_core::tracing::TraceSpanTimer,
    name: &'static str,
    status: &'static str,
) {
    if let Err(error) = trace_context
        .record_span(
            trace_recorder,
            timer,
            name,
            "runtime",
            status,
            std::collections::HashMap::new(),
        )
        .await
    {
        tracing::warn!(error = ?error, span = name, "failed to record trace span");
    }
}
