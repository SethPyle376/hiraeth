mod eval;
mod policy;
mod principal;
mod util;

pub use eval::PolicyEvalResult;
pub use eval::evaluate_policy;
pub use policy::Policy;
pub use principal::PolicyPrincipal;

#[derive(Debug)]
pub struct AuthorizationCheck {
    pub action: String,
    pub resource: String,
    pub resource_policy: Option<Policy>,
}
