mod create_access_key;
mod create_user;
mod get_user;
mod util;

use hiraeth_core::AwsActionRegistry;
use hiraeth_store::IamStore;

use crate::actions::{
    create_access_key::CreateAccessKeyAction, create_user::CreateUserAction,
    get_user::GetUserAction,
};

pub(crate) fn registry<S>() -> AwsActionRegistry<S>
where
    S: IamStore + Send + Sync + 'static,
{
    let mut registry = AwsActionRegistry::new();
    registry.register(Box::new(CreateAccessKeyAction));
    registry.register(Box::new(CreateUserAction));
    registry.register(Box::new(GetUserAction));
    registry
}
