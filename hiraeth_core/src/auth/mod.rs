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
