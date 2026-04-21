use async_trait::async_trait;
use hiraeth_auth::ResolvedRequest;
use hiraeth_core::{ServiceResponse, auth::AuthorizationCheck};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthorizationResult {
    Allow,
    Deny,
}

#[async_trait]
pub trait Authorizer {
    async fn authorize(
        &self,
        request: &ResolvedRequest,
        check: &AuthorizationCheck,
    ) -> AuthorizationResult;

    fn unauthorized_response(&self) -> ServiceResponse;
}
