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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthorizationOutcome {
    pub result: AuthorizationResult,
    pub trace_context: TraceContext,
}

impl AuthorizationOutcome {
    pub fn new(result: AuthorizationResult, trace_context: TraceContext) -> Self {
        Self {
            result,
            trace_context,
        }
    }
}

impl PartialEq<AuthorizationResult> for AuthorizationOutcome {
    fn eq(&self, other: &AuthorizationResult) -> bool {
        self.result == *other
    }
}

#[async_trait]
pub trait Authorizer {
    async fn authorize(
        &self,
        request: &ResolvedRequest,
        check: &AuthorizationCheck,
        trace_context: &TraceContext,
        trace_recorder: &dyn TraceRecorder,
    ) -> AuthorizationOutcome;

    fn unauthorized_response(&self) -> ServiceResponse;
}
