use hiraeth_core::tracing::{CompletedRequestTrace, TraceContext, TraceRecorder};
use hiraeth_http::IncomingRequest;
use hiraeth_iam::{AuthorizationMode, IamService};
use hiraeth_router::ServiceRouter;
use hiraeth_sns::SnsService;
use hiraeth_sqs::SqsService;
use hiraeth_store_sqlx::{SqliteIamStore, SqliteTraceStore, SqlxStore};
use hiraeth_sts::StsService;

use crate::request::{self, AppRequestOutcome};

pub struct App {
    iam: IamService<SqliteIamStore>,
    router: ServiceRouter,
    trace_recorder: SqliteTraceStore,
}

impl App {
    pub fn new(store: SqlxStore, auth_mode: AuthorizationMode) -> Self {
        let iam_store = store.iam_store.clone();
        let iam = IamService::new(auth_mode.clone(), iam_store.clone());
        let mut router = ServiceRouter::new(Box::new(IamService::new(
            auth_mode.clone(),
            iam_store.clone(),
        )));
        router.register_service(Box::new(IamService::new(auth_mode.clone(), iam_store)));
        router.register_service(Box::new(SqsService::new(store.sqs_store.clone())));
        router.register_service(Box::new(SnsService::new(
            store.sns_store.clone(),
            store.sqs_store.clone(),
            auth_mode.clone(),
        )));
        router.register_service(Box::new(StsService::new(store.iam_store.clone())));

        Self {
            iam,
            router,
            trace_recorder: store.trace_store,
        }
    }

    pub async fn handle_request(
        &self,
        trace_context: &TraceContext,
        incoming_request: IncomingRequest,
    ) -> AppRequestOutcome {
        request::resolve_and_route(
            trace_context,
            &self.trace_recorder,
            incoming_request,
            &self.iam,
            &self.router,
        )
        .await
    }

    pub async fn record_trace(&self, trace: CompletedRequestTrace) {
        if let Err(error) = self.trace_recorder.record_request_trace(trace).await {
            tracing::warn!(error = ?error, "failed to record request trace");
        }
    }
}
