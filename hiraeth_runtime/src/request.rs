use std::time::Instant;

use hiraeth_core::{ApiError, ServiceResponse};
use hiraeth_http::IncomingRequest;
use hiraeth_router::ServiceRouter;
use hiraeth_store_sqlx::SqlxStore;

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
    incoming_request: IncomingRequest,
    store: &SqlxStore,
    router: &ServiceRouter,
) -> AppRequestOutcome {
    let auth_started_at = Instant::now();
    let resolved_request = hiraeth_auth::resolve_request(incoming_request, &store.iam_store)
        .await
        .map_err(ApiError::from);

    let auth_ms = auth_started_at.elapsed().as_millis();

    match resolved_request {
        Ok(resolved_request) => {
            let trace = RequestTrace {
                auth_ms,
                route_ms: None,
                service: Some(resolved_request.service.clone()),
                region: Some(resolved_request.region.clone()),
                account_id: Some(resolved_request.auth_context.principal.account_id.clone()),
                principal: Some(resolved_request.auth_context.principal.name.clone()),
                access_key: Some(resolved_request.auth_context.access_key.clone()),
            };

            let route_started_at = Instant::now();
            let response = router.route(resolved_request).await;
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
