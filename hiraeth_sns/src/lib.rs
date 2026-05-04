use async_trait::async_trait;
use hiraeth_core::{
    ApiError, AuthContext, AwsActionRegistry, ResolvedRequest, ServiceResponse,
    auth::AuthorizationCheck,
    tracing::{TraceContext, TraceRecorder},
};
use hiraeth_iam::AuthorizationMode;
use hiraeth_router::Service;
use hiraeth_store::sns::SnsStore;
use hiraeth_store::sqs::SqsStore;

mod actions;
mod auth;
pub mod error;
mod store;

pub use store::SnsServiceStore;

pub struct SnsService<SS, QS> {
    store: SnsServiceStore<SS, QS>,
    actions: AwsActionRegistry<SnsServiceStore<SS, QS>>,
}

impl<SS, QS> SnsService<SS, QS>
where
    SS: SnsStore + Send + Sync + 'static,
    QS: SqsStore + Send + Sync + 'static,
{
    pub fn new(sns_store: SS, sqs_store: QS, auth_mode: AuthorizationMode) -> Self {
        Self {
            store: SnsServiceStore::new(sns_store, sqs_store, auth_mode),
            actions: actions::registry(),
        }
    }
}

#[async_trait]
impl<SS, QS> Service for SnsService<SS, QS>
where
    SS: SnsStore + Send + Sync,
    QS: SqsStore + Send + Sync,
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
        let action_name = match auth::get_action_name_for_request(&request) {
            Ok(action_name) => action_name,
            Err(error) => return Ok(ServiceResponse::from(error)),
        };

        let response = match self
            .actions
            .handle(
                &action_name,
                request,
                &self.store,
                trace_context,
                trace_recorder,
            )
            .await
        {
            Some(response) => response,
            None => {
                return Ok(ServiceResponse::from(
                    error::SnsError::UnsupportedOperation(action_name),
                ));
            }
        };

        Ok(response)
    }

    async fn resolve_authorization(
        &self,
        request: &ResolvedRequest,
    ) -> Result<AuthorizationCheck, ServiceResponse> {
        let action_name =
            auth::get_action_name_for_request(request).map_err(ServiceResponse::from)?;
        let action = self.actions.get(&action_name).ok_or_else(|| {
            ServiceResponse::from(error::SnsError::UnsupportedOperation(action_name.clone()))
        })?;

        action.resolve_authorization(request, &self.store).await
    }

    async fn validate_request(
        &self,
        request: &ResolvedRequest,
        trace_context: &TraceContext,
        trace_recorder: &dyn TraceRecorder,
    ) -> Result<(), ServiceResponse> {
        let action_name =
            auth::get_action_name_for_request(request).map_err(ServiceResponse::from)?;
        self.actions
            .validate(
                &action_name,
                request,
                &self.store,
                trace_context,
                trace_recorder,
            )
            .await
            .ok_or_else(|| {
                ServiceResponse::from(error::SnsError::UnsupportedOperation(action_name.clone()))
            })?
    }
}
