use hiraeth_core::AwsActionRegistry;
use hiraeth_store::IamStore;

use crate::actions::get_caller_identity::GetCallerIdentityAction;

mod get_caller_identity;
mod util;

pub(crate) fn registry<S>() -> AwsActionRegistry<S>
where
    S: IamStore + Send + Sync + 'static,
{
    let mut registry = AwsActionRegistry::new();
    registry.register(GetCallerIdentityAction);
    registry
}
