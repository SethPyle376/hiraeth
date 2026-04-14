use std::time::Instant;

use hiraeth_core::{ApiError, Config};
use hiraeth_http::IncomingRequest;
use hiraeth_router::{ServiceResponse, ServiceRouter};
use hiraeth_sqs::SqsService;
use hiraeth_store_sqlx::SqlxStore;

pub(crate) struct RequestTrace {
    pub auth_ms: u128,
    pub route_ms: Option<u128>,
    pub service: Option<String>,
    pub region: Option<String>,
    pub account_id: Option<String>,
    pub principal: Option<String>,
    pub access_key: Option<String>,
}

pub(crate) struct AppRequestOutcome {
    pub response: Result<ServiceResponse, ApiError>,
    pub trace: RequestTrace,
}

pub(crate) struct App {
    store: SqlxStore,
    router: ServiceRouter,
    config: Config,
}

impl App {
    pub async fn new(config: &Config) -> Result<Self, hiraeth_store_sqlx::StoreError> {
        let store = SqlxStore::new(&config.database_url).await?;

        let mut router = ServiceRouter::default();
        router.register_service(Box::new(SqsService::new(store.sqs_store.clone())));

        Ok(Self {
            store,
            router,
            config: config.clone(),
        })
    }

    pub async fn handle_request(&self, incoming_request: IncomingRequest) -> AppRequestOutcome {
        let auth_started_at = Instant::now();
        let resolved_request = hiraeth_auth::resolve_request(
            incoming_request,
            &self.store.access_key_store,
            &self.store.principal_store,
        )
        .await
        .map_err(ApiError::from);

        match resolved_request {
            Ok(resolved_request) => {
                let auth_elapsed = auth_started_at.elapsed();
                let trace = RequestTrace {
                    auth_ms: auth_elapsed.as_millis(),
                    route_ms: None,
                    service: Some(resolved_request.service.clone()),
                    region: Some(resolved_request.region.clone()),
                    account_id: Some(resolved_request.auth_context.principal.account_id.clone()),
                    principal: Some(resolved_request.auth_context.principal.name.clone()),
                    access_key: Some(resolved_request.auth_context.access_key.clone()),
                };

                let route_started_at = Instant::now();
                let routed_response = self.router.route(resolved_request).await;
                let route_elapsed = route_started_at.elapsed();

                AppRequestOutcome {
                    response: routed_response,
                    trace: RequestTrace {
                        route_ms: Some(route_elapsed.as_millis()),
                        ..trace
                    },
                }
            }
            Err(error) => {
                let auth_elapsed = auth_started_at.elapsed();
                AppRequestOutcome {
                    response: Err(error),
                    trace: RequestTrace {
                        auth_ms: auth_elapsed.as_millis(),
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
}
