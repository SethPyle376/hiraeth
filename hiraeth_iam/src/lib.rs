use async_trait::async_trait;
use hiraeth_auth::ResolvedRequest;
use hiraeth_core::{AuthMode, ServiceResponse, auth::AuthorizationCheck};
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
    _store: S,
}

impl<S: IamStore> IamService<S> {
    pub fn new(mode: AuthorizationMode, store: S) -> Self {
        Self {
            mode,
            _store: store,
        }
    }
}

impl From<AuthMode> for AuthorizationMode {
    fn from(value: AuthMode) -> Self {
        match value {
            AuthMode::Enforce => AuthorizationMode::Enforce,
            AuthMode::Audit => AuthorizationMode::Audit,
            AuthMode::Off => AuthorizationMode::Off,
        }
    }
}

#[async_trait]
impl<S> Service for IamService<S>
where
    S: IamStore + Send + Sync,
{
    fn can_handle(&self, _request: &ResolvedRequest) -> bool {
        false
    }

    async fn handle_request(
        &self,
        _request: ResolvedRequest,
    ) -> Result<ServiceResponse, hiraeth_core::ApiError> {
        todo!()
    }

    async fn resolve_authorization(
        &self,
        _request: &ResolvedRequest,
    ) -> Result<AuthorizationCheck, ServiceResponse> {
        todo!()
    }
}
