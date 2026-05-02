use async_trait::async_trait;
use hiraeth_core::{
    ApiError, AwsActionRegistry, ResolvedRequest, ServiceResponse,
    auth::AuthorizationCheck,
    tracing::{TraceContext, TraceRecorder},
};
use hiraeth_router::Service;
use hiraeth_store::sns::SnsStore;

mod actions;
mod delivery_target;
mod publish_utils;

pub struct SnsService<S: SnsStore> {
    store: S,
    actions: AwsActionRegistry<S>,
}

impl<S> SnsService<S>
where
    S: SnsStore + Send + Sync,
{
    pub fn new(store: S) -> Self {
        Self {
            store,
            actions: actions::registry(),
        }
    }
}

#[async_trait]
impl<S> Service for SnsService<S>
where
    S: SnsStore + Send + Sync,
{
    fn can_handle(&self, request: &ResolvedRequest) -> bool {
        request.service == "sns"
    }

    async fn handle_request(
        &self,
        request: ResolvedRequest,
        trace_context: &TraceContext,
        trace_recorder: &dyn TraceRecorder,
    ) -> Result<ServiceResponse, ApiError> {
        todo!()
    }

    async fn resolve_authorization(
        &self,
        request: &ResolvedRequest,
    ) -> Result<AuthorizationCheck, ServiceResponse> {
        todo!()
    }

    async fn validate_request(
        &self,
        request: &ResolvedRequest,
        trace_context: &TraceContext,
        trace_recorder: &dyn TraceRecorder,
    ) -> Result<(), ServiceResponse> {
        todo!()
    }
}
