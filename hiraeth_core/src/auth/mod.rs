mod eval;
mod policy;
mod principal;
mod util;

pub use eval::PolicyEvalResult;
pub use eval::evaluate_identity_policy;
pub use eval::evaluate_resource_policy;
pub use policy::Policy;
pub use principal::PolicyPrincipal;

#[derive(Debug)]
pub struct AuthorizationCheck {
    pub action: String,
    pub resource: String,
    pub resource_policy: Option<Policy>,
}

/// Evaluate a resource policy for a cross-service call where the caller is an
/// AWS service principal rather than an IAM user.
pub fn authorize_cross_service(
    caller: &PolicyPrincipal,
    action: &str,
    resource: &str,
    resource_policy: &Policy,
) -> PolicyEvalResult {
    evaluate_resource_policy(caller, resource, action, resource_policy)
}
