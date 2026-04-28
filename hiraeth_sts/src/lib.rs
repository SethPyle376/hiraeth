use async_trait::async_trait;
use hiraeth_core::{
    ApiError, AwsActionRegistry, ResolvedRequest, ServiceResponse, auth::AuthorizationCheck,
    get_query_request_action_name, parse_aws_query_params,
};
use hiraeth_router::Service;
use hiraeth_store::IamStore;

use crate::error::StsError;

mod actions;
mod error;

pub struct StsService<S: IamStore> {
    store: S,
    actions: AwsActionRegistry<S>,
}

impl<S> StsService<S>
where
    S: IamStore + Send + Sync + 'static,
{
    pub fn new(store: S) -> Self {
        Self {
            store,
            actions: actions::registry(),
        }
    }
}

#[async_trait]
impl<S> Service for StsService<S>
where
    S: IamStore + Send + Sync + 'static,
{
    fn can_handle(&self, request: &ResolvedRequest) -> bool {
        request.service == "sts"
    }

    async fn handle_request(&self, request: ResolvedRequest) -> Result<ServiceResponse, ApiError> {
        let action_name = get_query_request_action_name(&request)
            .map_err(|error| ApiError::BadRequest(error.to_string()))?
            .ok_or_else(|| ApiError::BadRequest("Missing Action parameter".to_string()))?;

        let action = match self.actions.get(&action_name) {
            Some(action) => action,
            None => {
                return Ok(ServiceResponse::from(
                    error::StsError::UnsupportedOperation(action_name.to_string()),
                ));
            }
        };

        Ok(action.handle(request, &self.store).await)
    }

    async fn resolve_authorization(
        &self,
        request: &ResolvedRequest,
    ) -> Result<AuthorizationCheck, ServiceResponse> {
        let action_name = get_query_request_action_name(request)
            .map_err(|error| ServiceResponse::from(StsError::from(error)))?
            .ok_or_else(|| {
                ServiceResponse::from(StsError::BadRequest("Missing Action parameter".to_string()))
            })?;
        let action = self.actions.get(&action_name).ok_or_else(|| {
            ServiceResponse::from(StsError::UnsupportedOperation(action_name.clone()))
        })?;

        action.resolve_authorization(request, &self.store).await
    }
}
