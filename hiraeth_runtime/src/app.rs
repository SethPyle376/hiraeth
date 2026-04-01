use hiraeth_core::ApiError;
use hiraeth_http::IncomingRequest;
use hiraeth_router::{ServiceResponse, ServiceRouter};
use hiraeth_sqs::SqsService;
use hiraeth_store_sqlx::SqlxStore;

pub(crate) struct App {
    store: SqlxStore,
    router: ServiceRouter,
}

impl App {
    pub async fn new(db_url: &str) -> Self {
        let mut router = ServiceRouter::default();
        router.register_service(Box::new(SqsService::new()));

        Self {
            store: SqlxStore::new(db_url)
                .await
                .inspect_err(|e| eprintln!("Failed to initialize store: {:?}", e))
                .expect("Store should be initialized"),
            router,
        }
    }

    pub async fn handle_request(
        &self,
        incoming_request: IncomingRequest,
    ) -> Result<ServiceResponse, ApiError> {
        let resolved_request =
            hiraeth_auth::resolve_request(incoming_request, &self.store.access_key_store)
                .await
                .map_err(|e| e.into())?;

        return self.router.route(resolved_request);
    }
}
