use async_trait::async_trait;
use hiraeth_core::{ApiError, ResolvedRequest, auth::AuthorizationCheck};

use crate::ServiceResponse;

#[async_trait]
pub trait Service {
    fn can_handle(&self, request: &ResolvedRequest) -> bool;
    async fn handle_request(&self, request: ResolvedRequest) -> Result<ServiceResponse, ApiError>;
    async fn resolve_authorization(
        &self,
        request: &ResolvedRequest,
    ) -> Result<AuthorizationCheck, ServiceResponse>;
}
