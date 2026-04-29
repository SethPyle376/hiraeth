use async_trait::async_trait;
use hiraeth_core::{
    ApiError, ResolvedRequest,
    auth::AuthorizationCheck,
    tracing::{TraceContext, TraceRecorder},
};

use crate::ServiceResponse;

#[async_trait]
pub trait Service {
    fn can_handle(&self, request: &ResolvedRequest) -> bool;
    async fn handle_request(
        &self,
        request: ResolvedRequest,
        trace_context: &TraceContext,
        trace_recorder: &dyn TraceRecorder,
    ) -> Result<ServiceResponse, ApiError>;
    async fn resolve_authorization(
        &self,
        request: &ResolvedRequest,
    ) -> Result<AuthorizationCheck, ServiceResponse>;
    async fn validate_request(
        &self,
        request: &ResolvedRequest,
        trace_context: &TraceContext,
        trace_recorder: &dyn TraceRecorder,
    ) -> Result<(), ServiceResponse>;
}
