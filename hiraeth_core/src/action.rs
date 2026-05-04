use async_trait::async_trait;
use serde::{Serialize, de::DeserializeOwned};

use crate::{
    AwsQueryParseError, RequestBodyParseError, ResolvedRequest, ResponseSerializationError,
    ServiceResponse,
    auth::AuthorizationCheck,
    empty_response, json_response, parse_aws_query_params, parse_aws_query_request,
    parse_json_body,
    tracing::{TraceContext, TraceRecorder},
    xml_response,
};

#[async_trait]
pub trait AwsAction<S>: Send + Sync {
    fn name(&self) -> &'static str;

    async fn validate(&self, request: &ResolvedRequest, store: &S) -> Result<(), ServiceResponse>;

    async fn handle(
        &self,
        request: ResolvedRequest,
        store: &S,
        trace_context: &TraceContext,
        trace_recorder: &dyn TraceRecorder,
    ) -> ServiceResponse;

    async fn resolve_authorization(
        &self,
        request: &ResolvedRequest,
        store: &S,
    ) -> Result<AuthorizationCheck, ServiceResponse>;
}

#[async_trait]
pub trait TypedAwsAction<S>: Send + Sync {
    type Request: DeserializeOwned + Send + Sync;
    type Response: Serialize + Send;
    type Error: From<ResponseSerializationError> + Into<ServiceResponse> + Send;

    fn name(&self) -> &'static str;

    fn payload_format(&self) -> AwsActionPayloadFormat {
        AwsActionPayloadFormat::AwsQuery
    }

    fn response_format(&self) -> AwsActionResponseFormat {
        AwsActionResponseFormat::Json
    }

    fn parse_error(&self, error: AwsActionPayloadParseError) -> Self::Error;

    async fn validate(
        &self,
        _request: &ResolvedRequest,
        _payload: &Self::Request,
        _store: &S,
    ) -> Result<(), Self::Error> {
        Ok(())
    }

    async fn handle(
        &self,
        request: ResolvedRequest,
        payload: Self::Request,
        store: &S,
        trace_context: &TraceContext,
        trace_recorder: &dyn TraceRecorder,
    ) -> Result<Self::Response, Self::Error>;

    async fn resolve_authorization(
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AwsActionResponseFormat {
    Json,
    Xml,
    Empty,
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

    async fn validate(&self, request: &ResolvedRequest, store: &S) -> Result<(), ServiceResponse> {
        let payload = parse_payload::<A::Request>(self.action.payload_format(), request)
            .map_err(|error| self.action.parse_error(error).into())?;

        self.action
            .validate(request, &payload, store)
            .await
            .map_err(Into::into)
    }

    async fn handle(
        &self,
        request: ResolvedRequest,
        store: &S,
        trace_context: &TraceContext,
        trace_recorder: &dyn TraceRecorder,
    ) -> ServiceResponse {
        let payload = match parse_payload::<A::Request>(self.action.payload_format(), &request) {
            Ok(payload) => payload,
            Err(error) => return self.action.parse_error(error).into(),
        };

        match self
            .action
            .handle(request, payload, store, trace_context, trace_recorder)
            .await
        {
            Ok(response) => match render_response(self.action.response_format(), &response) {
                Ok(response) => response,
                Err(error) => A::Error::from(error).into(),
            },
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
            .resolve_authorization(request, payload, store)
            .await
            .map_err(Into::into)
    }
}

fn render_response<T>(
    format: AwsActionResponseFormat,
    response: &T,
) -> Result<ServiceResponse, ResponseSerializationError>
where
    T: Serialize,
{
    match format {
        AwsActionResponseFormat::Json => json_response(response),
        AwsActionResponseFormat::Xml => xml_response(response),
        AwsActionResponseFormat::Empty => Ok(empty_response()),
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

    pub fn register<A>(&mut self, action: A)
    where
        S: Send + Sync + 'static,
        A: TypedAwsAction<S> + 'static,
    {
        self.actions
            .push(Box::new(TypedAwsActionAdapter::new(action)));
    }

    pub fn get(&self, name: &str) -> Option<&dyn AwsAction<S>> {
        self.actions
            .iter()
            .find(|action| action.name() == name)
            .map(|action| action.as_ref())
    }

    pub async fn handle(
        &self,
        action_name: &str,
        request: ResolvedRequest,
        store: &S,
        trace_context: &TraceContext,
        trace_recorder: &dyn TraceRecorder,
    ) -> Option<ServiceResponse> {
        let action = self.get(action_name)?;
        let timer = trace_context.start_span();
        let service = request.service.clone();
        let region = request.region.clone();
        let account_id = request.auth_context.principal.account_id.clone();
        let principal = request.auth_context.principal.name.clone();
        let action_name = action.name();
        let action_trace_context = trace_context.child_context(&timer);

        let response = action
            .handle(request, store, &action_trace_context, trace_recorder)
            .await;
        let status_code = response.status_code;
        let status = if status_code >= 400 { "error" } else { "ok" };

        if let Err(error) = trace_context
            .record_span(
                trace_recorder,
                timer,
                "action.handle",
                "action",
                status,
                std::collections::HashMap::from([
                    ("service".to_string(), service.clone()),
                    ("action".to_string(), format!("{service}:{action_name}")),
                    ("action_name".to_string(), action_name.to_string()),
                    ("region".to_string(), region),
                    ("account_id".to_string(), account_id),
                    ("principal".to_string(), principal),
                    ("status_code".to_string(), status_code.to_string()),
                ]),
            )
            .await
        {
            tracing::warn!(error = ?error, span = "action.handle", "failed to record trace span");
        }

        Some(response)
    }

    pub async fn validate(
        &self,
        action_name: &str,
        request: &ResolvedRequest,
        store: &S,
        _trace_context: &TraceContext,
        _trace_recorder: &dyn TraceRecorder,
    ) -> Option<Result<(), ServiceResponse>> {
        let action = self.get(action_name)?;
        let result = action.validate(request, store).await;
        Some(result)
    }
}

impl<S> Default for AwsActionRegistry<S> {
    fn default() -> Self {
        Self::new()
    }
}

pub fn get_query_request_action_name(
    request: &ResolvedRequest,
) -> Result<Option<String>, AwsQueryParseError> {
    let params = parse_aws_query_params(&request.request)?;
    Ok(params.get("Action").map(|s| s.to_string()))
}
