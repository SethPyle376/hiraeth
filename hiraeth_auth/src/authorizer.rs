use async_trait::async_trait;
use hiraeth_core::{
    ServiceResponse,
    auth::{AuthorizationCheck, PolicyPrincipal},
};

use crate::ResolvedRequest;

#[derive(Debug, PartialEq, Eq)]
pub enum AuthorizationResult {
    Allow,
    Deny,
}

#[async_trait]
pub trait Authorizer {
    async fn get_principal(&self, request: &ResolvedRequest) -> Option<PolicyPrincipal>;
    async fn authorize_request(
        &self,
        check: &AuthorizationCheck,
        principal: &PolicyPrincipal,
    ) -> AuthorizationResult;
    fn unauthorized_response(&self) -> ServiceResponse;
}
