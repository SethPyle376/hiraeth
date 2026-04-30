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
            .resolve_authorization_typed(request, payload, store)
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

#[cfg(test)]
mod tests {
    use std::{collections::HashMap, sync::Mutex};

    use async_trait::async_trait;
    use chrono::{TimeZone, Utc};

    use crate::{
        AuthContext, AwsAction, AwsActionRegistry, ResolvedRequest, ServiceResponse,
        auth::AuthorizationCheck,
        tracing::{
            CompletedRequestTrace, TraceContext, TraceRecordError, TraceRecorder, TraceSpanRecord,
        },
    };
    use hiraeth_http::IncomingRequest;
    use hiraeth_store::principal::Principal;

    struct TestAction(&'static str);

    #[async_trait]
    impl AwsAction<()> for TestAction {
        fn name(&self) -> &'static str {
            self.0
        }

        async fn validate(
            &self,
            _request: &ResolvedRequest,
            _store: &(),
        ) -> Result<(), ServiceResponse> {
            Ok(())
        }

        async fn handle(
            &self,
            _request: ResolvedRequest,
            _store: &(),
            _trace_context: &TraceContext,
            _trace_recorder: &dyn TraceRecorder,
        ) -> ServiceResponse {
            ServiceResponse {
                status_code: 200,
                body: Vec::new(),
                headers: Vec::new(),
            }
        }

        async fn resolve_authorization(
            &self,
            _request: &ResolvedRequest,
            _store: &(),
        ) -> Result<AuthorizationCheck, ServiceResponse> {
            unreachable!("test action registry does not execute auth checks")
        }
    }

    struct ChildSpanAction;

    #[async_trait]
    impl AwsAction<()> for ChildSpanAction {
        fn name(&self) -> &'static str {
            "SendMessage"
        }

        async fn validate(
            &self,
            _request: &ResolvedRequest,
            _store: &(),
        ) -> Result<(), ServiceResponse> {
            Ok(())
        }

        async fn handle(
            &self,
            _request: ResolvedRequest,
            _store: &(),
            trace_context: &TraceContext,
            trace_recorder: &dyn TraceRecorder,
        ) -> ServiceResponse {
            let timer = trace_context.start_span();
            trace_context
                .record_span(
                    trace_recorder,
                    timer,
                    "sqs.send_message.persist",
                    "sqs",
                    "ok",
                    HashMap::new(),
                )
                .await
                .expect("child span should record");

            ServiceResponse {
                status_code: 200,
                body: Vec::new(),
                headers: Vec::new(),
            }
        }

        async fn resolve_authorization(
            &self,
            _request: &ResolvedRequest,
            _store: &(),
        ) -> Result<AuthorizationCheck, ServiceResponse> {
            unreachable!("test action registry does not execute auth checks")
        }
    }

    #[derive(Default)]
    struct RecordingTraceRecorder {
        spans: Mutex<Vec<TraceSpanRecord>>,
    }

    #[async_trait]
    impl TraceRecorder for RecordingTraceRecorder {
        async fn record_request_trace(
            &self,
            _trace: CompletedRequestTrace,
        ) -> Result<(), TraceRecordError> {
            unreachable!("action registry tests only record spans")
        }

        async fn record_span(&self, span: TraceSpanRecord) -> Result<(), TraceRecordError> {
            self.spans
                .lock()
                .expect("trace recorder mutex should not be poisoned")
                .push(span);
            Ok(())
        }
    }

    fn resolved_request() -> ResolvedRequest {
        ResolvedRequest {
            request_id: "test-request-id".to_string(),
            request: IncomingRequest {
                host: "sqs.us-east-1.amazonaws.com".to_string(),
                method: "POST".to_string(),
                path: "/".to_string(),
                query: None,
                headers: HashMap::new(),
                body: Vec::new(),
            },
            service: "sqs".to_string(),
            region: "us-east-1".to_string(),
            auth_context: AuthContext {
                access_key: "AKIAIOSFODNN7EXAMPLE".to_string(),
                principal: Principal {
                    id: 1,
                    account_id: "123456789012".to_string(),
                    kind: "user".to_string(),
                    name: "test-user".to_string(),
                    path: "/".to_string(),
                    user_id: "AIDATESTUSER000001".to_string(),
                    created_at: Utc
                        .with_ymd_and_hms(2026, 4, 28, 12, 0, 0)
                        .unwrap()
                        .naive_utc(),
                },
            },
            date: Utc.with_ymd_and_hms(2026, 4, 28, 12, 0, 0).unwrap(),
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

    #[tokio::test]
    async fn registry_records_action_handle_span() {
        let mut registry = AwsActionRegistry::new();
        registry.register(Box::new(TestAction("SendMessage")));
        let trace_recorder = RecordingTraceRecorder::default();
        let trace_context = TraceContext::new("trace-request-id");

        let response = registry
            .handle(
                "SendMessage",
                resolved_request(),
                &(),
                &trace_context,
                &trace_recorder,
            )
            .await
            .expect("registered action should be handled");

        assert_eq!(response.status_code, 200);

        let spans = trace_recorder
            .spans
            .lock()
            .expect("trace recorder mutex should not be poisoned");
        assert_eq!(spans.len(), 1);

        let span = &spans[0];
        assert_eq!(span.name, "action.handle");
        assert_eq!(span.layer, "action");
        assert_eq!(span.status, "ok");
        assert_eq!(
            span.attributes.get("action").map(String::as_str),
            Some("sqs:SendMessage")
        );
        assert_eq!(
            span.attributes.get("action_name").map(String::as_str),
            Some("SendMessage")
        );
        assert_eq!(
            span.attributes.get("status_code").map(String::as_str),
            Some("200")
        );
    }

    #[tokio::test]
    async fn registry_passes_action_span_as_child_context() {
        let mut registry = AwsActionRegistry::new();
        registry.register(Box::new(ChildSpanAction));
        let trace_recorder = RecordingTraceRecorder::default();
        let trace_context = TraceContext::new("trace-request-id");

        let response = registry
            .handle(
                "SendMessage",
                resolved_request(),
                &(),
                &trace_context,
                &trace_recorder,
            )
            .await
            .expect("registered action should be handled");

        assert_eq!(response.status_code, 200);

        let spans = trace_recorder
            .spans
            .lock()
            .expect("trace recorder mutex should not be poisoned");
        assert_eq!(spans.len(), 2);

        let child_span = spans
            .iter()
            .find(|span| span.name == "sqs.send_message.persist")
            .expect("child action span should be recorded");
        let action_span = spans
            .iter()
            .find(|span| span.name == "action.handle")
            .expect("action handle span should be recorded");

        assert_eq!(
            child_span.parent_span_id.as_deref(),
            Some(action_span.span_id.as_str())
        );
    }
}
