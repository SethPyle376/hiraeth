use hiraeth_http::IncomingRequest;
use hiraeth_router::ServiceRouter;
use hiraeth_sqs::SqsService;
use hiraeth_store_sqlx::SqlxStore;

use crate::request::{self, AppRequestOutcome};

pub struct App {
    store: SqlxStore,
    router: ServiceRouter,
}

impl App {
    pub fn new(store: SqlxStore) -> Self {
        let mut router = ServiceRouter::default();
        router.register_service(Box::new(SqsService::new(store.sqs_store.clone())));

        Self { store, router }
    }

    pub async fn handle_request(&self, incoming_request: IncomingRequest) -> AppRequestOutcome {
        request::resolve_and_route(incoming_request, &self.store, &self.router).await
    }
}
