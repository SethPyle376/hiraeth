mod policy;
mod principal;
mod util;

pub use policy::Policy;

#[derive(Debug)]
pub struct AuthorizationCheck {
    pub action: String,
    pub resource: String,
    pub resource_policy: Option<Policy>,
}
