mod attach_user_policy;
mod create_access_key;
mod create_policy;
mod create_user;
mod delete_policy;
mod delete_user;
mod detach_user_policy;
mod get_policy;
mod get_policy_version;
mod get_user;
mod get_user_policy;
mod list_access_keys;
mod put_user_policy;
mod util;

use hiraeth_core::AwsActionRegistry;
use hiraeth_store::IamStore;

use crate::actions::{
    attach_user_policy::AttachUserPolicyAction, create_access_key::CreateAccessKeyAction,
    create_policy::CreatePolicyAction, create_user::CreateUserAction,
    delete_policy::DeletePolicyAction, delete_user::DeleteUserAction,
    detach_user_policy::DetachUserPolicyAction, get_policy::GetPolicyAction,
    get_policy_version::GetPolicyVersionAction, get_user::GetUserAction,
    get_user_policy::GetUserPolicyAction, list_access_keys::ListAccessKeysAction,
    put_user_policy::PutUserPolicyAction,
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
    registry.register_typed(CreatePolicyAction);
    registry.register_typed(PutUserPolicyAction);
    registry.register_typed(AttachUserPolicyAction);
    registry.register_typed(DetachUserPolicyAction);
    registry.register_typed(DeletePolicyAction);
    registry.register_typed(GetUserPolicyAction);
    registry.register_typed(GetPolicyAction);
    registry.register_typed(GetPolicyVersionAction);
    registry.register_typed(ListAccessKeysAction);
    registry
}
