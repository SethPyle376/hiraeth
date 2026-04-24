use async_trait::async_trait;
use serde::de::DeserializeOwned;

use crate::{
    AwsQueryParseError, RequestBodyParseError, ResolvedRequest, ServiceResponse,
    auth::AuthorizationCheck, parse_aws_query_request, parse_json_body,
};

#[async_trait]
pub trait AwsAction<S>: Send + Sync {
    fn name(&self) -> &'static str;

    async fn handle(&self, request: ResolvedRequest, store: &S) -> ServiceResponse;

    async fn resolve_authorization(
        &self,
        request: &ResolvedRequest,
        store: &S,
    ) -> Result<AuthorizationCheck, ServiceResponse>;
}

#[async_trait]
pub trait TypedAwsAction<S>: Send + Sync {
    type Request: DeserializeOwned + Send;
    type Error: Into<ServiceResponse> + Send;

    fn name(&self) -> &'static str;

    fn payload_format(&self) -> AwsActionPayloadFormat {
        AwsActionPayloadFormat::AwsQuery
    }

    fn parse_error(&self, error: AwsActionPayloadParseError) -> Self::Error;

    async fn handle_typed(
        &self,
        request: ResolvedRequest,
        payload: Self::Request,
        store: &S,
    ) -> Result<ServiceResponse, Self::Error>;

    async fn resolve_authorization_typed(
        &self,
        request: &ResolvedRequest,
        payload: Self::Request,
        store: &S,
    ) -> Result<AuthorizationCheck, Self::Error>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AwsActionPayloadFormat {
    AwsQuery,
    Json,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AwsActionPayloadParseError {
    AwsQuery(AwsQueryParseError),
    Json(RequestBodyParseError),
}

pub struct TypedAwsActionAdapter<A> {
    action: A,
}

impl<A> TypedAwsActionAdapter<A> {
    pub fn new(action: A) -> Self {
        Self { action }
    }
}

#[async_trait]
impl<S, A> AwsAction<S> for TypedAwsActionAdapter<A>
where
    S: Send + Sync,
    A: TypedAwsAction<S>,
{
    fn name(&self) -> &'static str {
        self.action.name()
    }

    async fn handle(&self, request: ResolvedRequest, store: &S) -> ServiceResponse {
        let payload = match parse_payload::<A::Request>(self.action.payload_format(), &request) {
            Ok(payload) => payload,
            Err(error) => return self.action.parse_error(error).into(),
        };

        match self.action.handle_typed(request, payload, store).await {
            Ok(response) => response,
            Err(error) => error.into(),
        }
    }

    async fn resolve_authorization(
        &self,
        request: &ResolvedRequest,
        store: &S,
    ) -> Result<AuthorizationCheck, ServiceResponse> {
        let payload = parse_payload::<A::Request>(self.action.payload_format(), request)
            .map_err(|error| self.action.parse_error(error).into())?;

        self.action
            .resolve_authorization_typed(request, payload, store)
            .await
            .map_err(Into::into)
    }
}

fn parse_payload<T>(
    format: AwsActionPayloadFormat,
    request: &ResolvedRequest,
) -> Result<T, AwsActionPayloadParseError>
where
    T: DeserializeOwned,
{
    match format {
        AwsActionPayloadFormat::AwsQuery => {
            parse_aws_query_request(&request.request).map_err(AwsActionPayloadParseError::AwsQuery)
        }
        AwsActionPayloadFormat::Json => {
            parse_json_body(&request.request.body).map_err(AwsActionPayloadParseError::Json)
        }
    }
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

    pub fn register_typed<A>(&mut self, action: A)
    where
        S: Send + Sync + 'static,
        A: TypedAwsAction<S> + 'static,
    {
        self.register(Box::new(TypedAwsActionAdapter::new(action)));
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

        async fn handle(&self, _request: ResolvedRequest, _store: &()) -> ServiceResponse {
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
