use async_trait::async_trait;
use hiraeth_auth::ResolvedRequest;
use hiraeth_core::{ServiceResponse, auth::AuthorizationCheck};
use hiraeth_router::Service;
use hiraeth_store::IamStore;

mod authorize;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AuthorizationMode {
    Enforce,
    Audit,
    Off,
}

#[derive(Debug, Clone)]
pub struct IamService<S: IamStore> {
    mode: AuthorizationMode,
    store: S,
}

impl<S: IamStore> IamService<S> {
    pub fn new(mode: AuthorizationMode, store: S) -> Self {
        Self { mode, store }
    }
}

#[async_trait]
impl<S> Service for IamService<S>
where
    S: IamStore + Send + Sync,
{
    fn can_handle(&self, request: &ResolvedRequest) -> bool {
        false
    }

    async fn handle_request(
        &self,
        request: ResolvedRequest,
    ) -> Result<ServiceResponse, hiraeth_core::ApiError> {
        todo!()
    }

    async fn auth_request(
        &self,
        request: &ResolvedRequest,
    ) -> Result<AuthorizationCheck, ServiceResponse> {
        todo!()
    }
}

