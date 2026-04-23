mod create_access_key;
mod create_user;
mod util;

use hiraeth_core::AwsActionRegistry;
use hiraeth_store::IamStore;

pub(crate) use create_access_key::CreateAccessKeyAction;
pub(crate) use create_user::CreateUserAction;

pub(crate) fn registry<S>() -> AwsActionRegistry<S>
where
    S: IamStore + Send + Sync + 'static,
{
    let mut registry = AwsActionRegistry::new();
    registry.register(Box::new(CreateAccessKeyAction));
    registry.register(Box::new(CreateUserAction));
    registry
}
