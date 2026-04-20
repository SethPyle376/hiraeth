use async_trait::async_trait;
use hiraeth_auth::{AuthorizationResult, Authorizer, ResolvedRequest};
use hiraeth_core::{
    ServiceResponse,
    auth::{AuthorizationCheck, PolicyPrincipal},
};
use hiraeth_store::IamStore;

use crate::{AuthorizationMode, IamService};

#[async_trait]
impl<S> Authorizer for IamService<S>
where
    S: IamStore + Send + Sync,
{
    async fn get_principal(&self, request: &ResolvedRequest) -> Option<PolicyPrincipal> {
        let request_principal = request.auth_context.principal.clone();
        match request_principal.kind.as_str() {
            "user" => Some(PolicyPrincipal::User {
                account_id: request_principal.account_id.clone(),
                user_name: request_principal.name.clone(),
            }),
            _ => None,
        }
    }

    async fn authorize_request(
        &self,
        check: &AuthorizationCheck,
        principal: &PolicyPrincipal,
    ) -> AuthorizationResult {
        let policy_result = check.resource_policy.as_ref().map(|policy| {
            hiraeth_core::auth::evaluate_policy(principal, &check.resource, &check.action, policy)
        });

        let authn_result = match policy_result {
            Some(hiraeth_core::auth::PolicyEvalResult::Allowed) => AuthorizationResult::Allow,
            Some(hiraeth_core::auth::PolicyEvalResult::Denied) => AuthorizationResult::Deny,
            _ => AuthorizationResult::Deny, // default to deny if no applicable policy
        };

        match self.mode {
            AuthorizationMode::Enforce => authn_result,
            AuthorizationMode::Audit => {
                tracing::info!(
                    "Audit authz: principal={:?}, resource={}, action={}, result={:?}",
                    principal,
                    check.resource,
                    check.action,
                    authn_result
                );
                AuthorizationResult::Allow // allow the request but log the result
            }
            AuthorizationMode::Off => AuthorizationResult::Allow, // allow all requests
        }
    }

    fn unauthorized_response(&self) -> ServiceResponse {
        ServiceResponse {
            status_code: 403,
            body: vec![],
            headers: vec![],
        }
    }
}
