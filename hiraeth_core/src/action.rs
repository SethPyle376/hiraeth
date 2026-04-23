use async_trait::async_trait;

use crate::{ApiError, ResolvedRequest, ServiceResponse, auth::AuthorizationCheck};

#[async_trait]
pub trait AwsAction<S>: Send + Sync {
    fn name(&self) -> &'static str;

    async fn handle(
        &self,
        request: ResolvedRequest,
        store: &S,
    ) -> Result<ServiceResponse, ApiError>;

    async fn resolve_authorization(
        &self,
        request: &ResolvedRequest,
        store: &S,
    ) -> Result<AuthorizationCheck, ServiceResponse>;
}

pub struct AwsActionRegistry<S> {
    actions: Vec<Box<dyn AwsAction<S>>>,
}

impl<S> AwsActionRegistry<S> {
    pub fn new() -> Self {
        Self {
            actions: Vec::new(),
        }
    }

    pub fn register(&mut self, action: Box<dyn AwsAction<S>>) {
        self.actions.push(action);
    }

    pub fn get(&self, name: &str) -> Option<&dyn AwsAction<S>> {
        self.actions
            .iter()
            .find(|action| action.name() == name)
            .map(|action| action.as_ref())
    }
}

impl<S> Default for AwsActionRegistry<S> {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use async_trait::async_trait;

    use crate::{
        AwsAction, AwsActionRegistry, ResolvedRequest, ServiceResponse, auth::AuthorizationCheck,
    };

    struct TestAction(&'static str);

    #[async_trait]
    impl AwsAction<()> for TestAction {
        fn name(&self) -> &'static str {
            self.0
        }

        async fn handle(
            &self,
            _request: ResolvedRequest,
            _store: &(),
        ) -> Result<ServiceResponse, crate::ApiError> {
            unreachable!("test action registry does not execute handlers")
        }

        async fn resolve_authorization(
            &self,
            _request: &ResolvedRequest,
            _store: &(),
        ) -> Result<AuthorizationCheck, ServiceResponse> {
            unreachable!("test action registry does not execute auth checks")
        }
    }

    #[test]
    fn registry_returns_action_by_name() {
        let mut registry = AwsActionRegistry::new();
        registry.register(Box::new(TestAction("CreateThing")));
        registry.register(Box::new(TestAction("DeleteThing")));

        assert_eq!(
            registry.get("DeleteThing").map(AwsAction::name),
            Some("DeleteThing")
        );
        assert!(registry.get("MissingThing").is_none());
    }
}
