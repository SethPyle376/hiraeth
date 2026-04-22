use hiraeth_http::IncomingRequest;
use hiraeth_iam::{AuthorizationMode, IamService};
use hiraeth_router::ServiceRouter;
use hiraeth_sqs::SqsService;
use hiraeth_store_sqlx::SqlxStore;

use crate::request::{self, AppRequestOutcome};

pub struct App {
    iam: IamService<hiraeth_store_sqlx::SqliteIamStore>,
    router: ServiceRouter,
}

impl App {
    pub fn new(store: SqlxStore, auth_mode: AuthorizationMode) -> Self {
        let iam = IamService::new(auth_mode, store.iam_store.clone());
        let mut router = ServiceRouter::new(Box::new(iam.clone()));
        router.register_service(Box::new(iam.clone()));
        router.register_service(Box::new(SqsService::new(store.sqs_store.clone())));

        Self { iam, router }
    }

    pub async fn handle_request(&self, incoming_request: IncomingRequest) -> AppRequestOutcome {
        request::resolve_and_route(incoming_request, &self.iam, &self.router).await
    }
}
