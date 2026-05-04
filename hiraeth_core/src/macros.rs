/// Implements [`TypedAwsAction`] for a unit-struct action, cutting out the ~60 lines of
/// boilerplate that every action currently duplicates.
///
/// # Supported patterns
///
/// ## Simple store (single generic parameter)
/// ```ignore
/// impl_aws_action! {
///     CreateTopicAction<S: SnsStore> {
///         request: CreateTopicRequest,
///         response: CreateTopicResponse,
///         error: SnsError,
///         name: "CreateTopic",
///         payload: AwsQuery,
///         response_format: Xml,
///         parse_error: parse_payload_error,
///         validate: |request, payload, store| {
///             if payload.name.is_empty() {
///                 return Err(SnsError::BadRequest("Name is required".to_string()));
///             }
///             Ok(())
///         },
///         handler: handle_create_topic_typed,
///         span_name: "sns.topic.create",
///         span_service: "sns",
///         span_attrs: |payload| {
///             HashMap::from([("topic_name".to_string(), payload.name.clone())])
///         },
///         authorize: |request, _payload, _store| {
///             Ok(AuthorizationCheck {
///                 action: "sns:CreateTopic".to_string(),
///                 resource: format!("arn:aws:sns:{}:{}:*", request.region, request.auth_context.principal.account_id),
///                 resource_policy: None,
///             })
///         },
///     }
/// }
/// ```
///
/// ## Composite store (multiple generics + explicit store type)
/// ```ignore
/// impl_aws_action! {
///     SetTopicAttributesAction<SnsServiceStore<SS, QS>> where SS: SnsStore, QS: SqsStore {
///         request: SetTopicAttributesRequest,
///         response: SetTopicAttributesResponse,
///         error: SnsError,
///         name: "SetTopicAttributes",
///         payload: AwsQuery,
///         response_format: Xml,
///         parse_error: parse_payload_error,
///         validate: |request, payload, store| {
///             if !is_valid_topic_attribute(&payload.attribute_name) {
///                 return Err(SnsError::BadRequest(format!(
///                     "Unsupported attribute name: {}",
///                     payload.attribute_name
///                 )));
///             }
///             Ok(())
///         },
///         handler: handle_set_topic_attributes,
///         span_name: "sns.topic_attributes.set",
///         span_service: "sns",
///         span_attrs: |payload| {
///             HashMap::from([
///                 ("topic_arn".to_string(), payload.topic_arn.clone()),
///                 ("attribute_name".to_string(), payload.attribute_name.clone()),
///                 ("attribute_value".to_string(), payload.attribute_value.clone()),
///             ])
///         },
///         authorize: |request, _payload, store| {
///             resolve_authorization("sns:SetTopicAttributes", request, &store.sns_store).await
///         },
///     }
/// }
/// ```
///
/// ## Custom `handle` body (for actions that need manual span recording)
/// Use `handle:` instead of `handler:` + `span_*` fields.
/// ```ignore
/// impl_aws_action! {
///     PublishAction<SnsServiceStore<SS, QS>> where SS: SnsStore, QS: SqsStore {
///         request: PublishRequest,
///         response: PublishResponse,
///         error: SnsError,
///         name: "Publish",
///         payload: AwsQuery,
///         response_format: Xml,
///         parse_error: parse_payload_error,
///         validate: |request, payload, store| { /* ... */ },
///         handle: |request, payload, store, trace_context, trace_recorder| {
///             let attributes = HashMap::from([/* ... */]);
///             let timer = trace_context.start_span();
///             let child = trace_context.child_context(&timer);
///             let result = handle_publish_typed(&request, store, payload, &child, trace_recorder).await;
///             let status = if result.is_ok() { "ok" } else { "error" };
///             trace_context.record_span_or_warn(trace_recorder, timer, "sns.message.publish", "sns", status, attributes).await;
///             result
///         },
///         authorize: |request, _payload, store| {
///             crate::auth::resolve_authorization("sns:Publish", request, &store.sns_store).await
///         },
///     }
/// }
/// ```
#[macro_export]
macro_rules! impl_aws_action {
    // ------------------------------------------------------------------
    // Simple store + standard handle (handler + span wrapping)
    // ------------------------------------------------------------------
    (
        $action:ident < $store:ident : $store_bound:path > {
            request: $request:ty,
            response: $response:ty,
            error: $error:ty,
            name: $name:literal,
            payload: $payload:ident,
            response_format: $response_format:ident,
            parse_error: $parse_error:path,
            $(validate: |$validate_req:ident, $validate_payload:ident, $validate_store:ident| $validate:block,)?
            handler: $handler:path,
            span_name: $span_name:literal,
            span_service: $span_service:literal,
            span_attrs: |$span_payload:ident| $span_attrs:block,
            authorize: |$auth_req:ident, $auth_payload:ident, $auth_store:ident| $authorize:block,
        }
    ) => {
        #[::async_trait::async_trait]
        impl<$store> $crate::TypedAwsAction<$store> for $action
        where
            $store: $store_bound + Send + Sync,
        {
            type Request = $request;
            type Response = $response;
            type Error = $error;

            fn name(&self) -> &'static str {
                $name
            }

            fn payload_format(&self) -> $crate::AwsActionPayloadFormat {
                $crate::AwsActionPayloadFormat::$payload
            }

            fn response_format(&self) -> $crate::AwsActionResponseFormat {
                $crate::AwsActionResponseFormat::$response_format
            }

            fn parse_error(&self, error: $crate::AwsActionPayloadParseError) -> Self::Error {
                $parse_error(error)
            }

            $(
                async fn validate(
                    &self,
                    $validate_req: &$crate::ResolvedRequest,
                    $validate_payload: &Self::Request,
                    $validate_store: &$store,
                ) -> Result<(), Self::Error> {
                    $validate
                }
            )?

            async fn handle(
                &self,
                request: $crate::ResolvedRequest,
                payload: Self::Request,
                store: &$store,
                trace_context: &$crate::tracing::TraceContext,
                trace_recorder: &dyn $crate::tracing::TraceRecorder,
            ) -> Result<Self::Response, Self::Error> {
                let $span_payload = &payload;
                let attributes = $span_attrs;
                trace_context
                    .record_result_span(
                        trace_recorder,
                        $span_name,
                        $span_service,
                        attributes,
                        async { $handler(&request, store, payload).await },
                    )
                    .await
            }

            async fn resolve_authorization(
                &self,
                $auth_req: &$crate::ResolvedRequest,
                $auth_payload: Self::Request,
                $auth_store: &$store,
            ) -> Result<$crate::auth::AuthorizationCheck, Self::Error> {
                $authorize
            }
        }
    };

    // ------------------------------------------------------------------
    // Simple store + custom handle body
    // ------------------------------------------------------------------
    (
        $action:ident < $store:ident : $store_bound:path > {
            request: $request:ty,
            response: $response:ty,
            error: $error:ty,
            name: $name:literal,
            payload: $payload:ident,
            response_format: $response_format:ident,
            parse_error: $parse_error:path,
            $(validate: |$validate_req:ident, $validate_payload:ident, $validate_store:ident| $validate:block,)?
            handle: |$handle_req:ident, $handle_payload:ident, $handle_store:ident, $handle_trace:ident, $handle_recorder:ident| $handle:block,
            authorize: |$auth_req:ident, $auth_payload:ident, $auth_store:ident| $authorize:block,
        }
    ) => {
        #[::async_trait::async_trait]
        impl<$store> $crate::TypedAwsAction<$store> for $action
        where
            $store: $store_bound + Send + Sync,
        {
            type Request = $request;
            type Response = $response;
            type Error = $error;

            fn name(&self) -> &'static str {
                $name
            }

            fn payload_format(&self) -> $crate::AwsActionPayloadFormat {
                $crate::AwsActionPayloadFormat::$payload
            }

            fn response_format(&self) -> $crate::AwsActionResponseFormat {
                $crate::AwsActionResponseFormat::$response_format
            }

            fn parse_error(&self, error: $crate::AwsActionPayloadParseError) -> Self::Error {
                $parse_error(error)
            }

            $(
                async fn validate(
                    &self,
                    $validate_req: &$crate::ResolvedRequest,
                    $validate_payload: &Self::Request,
                    $validate_store: &$store,
                ) -> Result<(), Self::Error> {
                    $validate
                }
            )?

            async fn handle(
                &self,
                $handle_req: $crate::ResolvedRequest,
                $handle_payload: Self::Request,
                $handle_store: &$store,
                $handle_trace: &$crate::tracing::TraceContext,
                $handle_recorder: &dyn $crate::tracing::TraceRecorder,
            ) -> Result<Self::Response, Self::Error> {
                $handle
            }

            async fn resolve_authorization(
                &self,
                $auth_req: &$crate::ResolvedRequest,
                $auth_payload: Self::Request,
                $auth_store: &$store,
            ) -> Result<$crate::auth::AuthorizationCheck, Self::Error> {
                $authorize
            }
        }
    };

    // ------------------------------------------------------------------
    // Composite store + standard handle
    // ------------------------------------------------------------------
    (
        $action:ident < $store_type:ty > where $($store_generic:ident : $store_generic_bound:path),+ $(,)? {
            request: $request:ty,
            response: $response:ty,
            error: $error:ty,
            name: $name:literal,
            payload: $payload:ident,
            response_format: $response_format:ident,
            parse_error: $parse_error:path,
            $(validate: |$validate_req:ident, $validate_payload:ident, $validate_store:ident| $validate:block,)?
            handler: $handler:path,
            span_name: $span_name:literal,
            span_service: $span_service:literal,
            span_attrs: |$span_payload:ident| $span_attrs:block,
            authorize: |$auth_req:ident, $auth_payload:ident, $auth_store:ident| $authorize:block,
        }
    ) => {
        #[::async_trait::async_trait]
        impl<$($store_generic),+> $crate::TypedAwsAction<$store_type> for $action
        where
            $($store_generic: $store_generic_bound + Send + Sync),+,
        {
            type Request = $request;
            type Response = $response;
            type Error = $error;

            fn name(&self) -> &'static str {
                $name
            }

            fn payload_format(&self) -> $crate::AwsActionPayloadFormat {
                $crate::AwsActionPayloadFormat::$payload
            }

            fn response_format(&self) -> $crate::AwsActionResponseFormat {
                $crate::AwsActionResponseFormat::$response_format
            }

            fn parse_error(&self, error: $crate::AwsActionPayloadParseError) -> Self::Error {
                $parse_error(error)
            }

            $(
                async fn validate(
                    &self,
                    $validate_req: &$crate::ResolvedRequest,
                    $validate_payload: &Self::Request,
                    $validate_store: &$store_type,
                ) -> Result<(), Self::Error> {
                    $validate
                }
            )?

            async fn handle(
                &self,
                request: $crate::ResolvedRequest,
                payload: Self::Request,
                store: &$store_type,
                trace_context: &$crate::tracing::TraceContext,
                trace_recorder: &dyn $crate::tracing::TraceRecorder,
            ) -> Result<Self::Response, Self::Error> {
                let $span_payload = &payload;
                let attributes = $span_attrs;
                trace_context
                    .record_result_span(
                        trace_recorder,
                        $span_name,
                        $span_service,
                        attributes,
                        async { $handler(&request, store, payload).await },
                    )
                    .await
            }

            async fn resolve_authorization(
                &self,
                $auth_req: &$crate::ResolvedRequest,
                $auth_payload: Self::Request,
                $auth_store: &$store_type,
            ) -> Result<$crate::auth::AuthorizationCheck, Self::Error> {
                $authorize
            }
        }
    };

    // ------------------------------------------------------------------
    // Composite store + custom handle body
    // ------------------------------------------------------------------
    (
        $action:ident < $store_type:ty > where $($store_generic:ident : $store_generic_bound:path),+ $(,)? {
            request: $request:ty,
            response: $response:ty,
            error: $error:ty,
            name: $name:literal,
            payload: $payload:ident,
            response_format: $response_format:ident,
            parse_error: $parse_error:path,
            $(validate: |$validate_req:ident, $validate_payload:ident, $validate_store:ident| $validate:block,)?
            handle: |$handle_req:ident, $handle_payload:ident, $handle_store:ident, $handle_trace:ident, $handle_recorder:ident| $handle:block,
            authorize: |$auth_req:ident, $auth_payload:ident, $auth_store:ident| $authorize:block,
        }
    ) => {
        #[::async_trait::async_trait]
        impl<$($store_generic),+> $crate::TypedAwsAction<$store_type> for $action
        where
            $($store_generic: $store_generic_bound + Send + Sync),+,
        {
            type Request = $request;
            type Response = $response;
            type Error = $error;

            fn name(&self) -> &'static str {
                $name
            }

            fn payload_format(&self) -> $crate::AwsActionPayloadFormat {
                $crate::AwsActionPayloadFormat::$payload
            }

            fn response_format(&self) -> $crate::AwsActionResponseFormat {
                $crate::AwsActionResponseFormat::$response_format
            }

            fn parse_error(&self, error: $crate::AwsActionPayloadParseError) -> Self::Error {
                $parse_error(error)
            }

            $(
                async fn validate(
                    &self,
                    $validate_req: &$crate::ResolvedRequest,
                    $validate_payload: &Self::Request,
                    $validate_store: &$store_type,
                ) -> Result<(), Self::Error> {
                    $validate
                }
            )?

            async fn handle(
                &self,
                $handle_req: $crate::ResolvedRequest,
                $handle_payload: Self::Request,
                $handle_store: &$store_type,
                $handle_trace: &$crate::tracing::TraceContext,
                $handle_recorder: &dyn $crate::tracing::TraceRecorder,
            ) -> Result<Self::Response, Self::Error> {
                $handle
            }

            async fn resolve_authorization(
                &self,
                $auth_req: &$crate::ResolvedRequest,
                $auth_payload: Self::Request,
                $auth_store: &$store_type,
            ) -> Result<$crate::auth::AuthorizationCheck, Self::Error> {
                $authorize
            }
        }
    };
}
