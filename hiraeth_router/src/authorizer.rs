use async_trait::async_trait;
use hiraeth_core::{
    ResolvedRequest, ServiceResponse,
    auth::AuthorizationCheck,
    tracing::{TraceContext, TraceRecorder},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthorizationResult {
    Allow,
    Deny,
}

impl AuthorizationResult {
    pub fn as_trace_status(self) -> &'static str {
        match self {
            AuthorizationResult::Allow => "allow",
            AuthorizationResult::Deny => "deny",
        }
    }
}

#[async_trait]
pub trait Authorizer {
    async fn authorize(
        &self,
        request: &ResolvedRequest,
        check: &AuthorizationCheck,
        trace_context: &TraceContext,
        trace_recorder: &(dyn TraceRecorder + Sync),
    ) -> AuthorizationResult;

    fn unauthorized_response(&self) -> ServiceResponse;
}
