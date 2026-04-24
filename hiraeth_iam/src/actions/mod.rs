mod create_access_key;
mod create_user;
mod delete_user;
mod get_user;
mod util;

use hiraeth_core::AwsActionRegistry;
use hiraeth_store::IamStore;

use crate::actions::{
    create_access_key::CreateAccessKeyAction, create_user::CreateUserAction,
    delete_user::DeleteUserAction, get_user::GetUserAction,
};

pub(crate) fn registry<S>() -> AwsActionRegistry<S>
where
    S: IamStore + Send + Sync + 'static,
{
    let mut registry = AwsActionRegistry::new();
    registry.register_typed(CreateAccessKeyAction);
    registry.register_typed(CreateUserAction);
    registry.register_typed(GetUserAction);
    registry.register_typed(DeleteUserAction);
    registry
}
