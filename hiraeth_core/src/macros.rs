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
///         span_attrs: |_request, payload, _store| {
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
///         span_attrs: |_request, payload, _store| {
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
/// ## Traced handler (for actions that need manual span recording)
/// Use `traced_handler:` instead of `handler:` + `span_*` fields.
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
///         traced_handler: handle_publish_typed,
///         authorize: |request, _payload, store| {
///             crate::auth::resolve_authorization("sns:Publish", request, &store.sns_store).await
///         },
///     }
/// }
/// ```
#[macro_export]
macro_rules! impl_aws_action {
    // ------------------------------------------------------------------
    // Simple store + standard handler (handler + span wrapping)
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
            span_attrs: |$span_req:ident, $span_payload:ident, $span_store:ident| $span_attrs:block,
            authorize: |$auth_req:ident, $auth_payload:ident, $auth_store:ident| $authorize:block,
        }
    ) => {
        $crate::impl_aws_action! {
            @standard
            generics: { $store },
            store_type: $store,
            where_clause: { $store: $store_bound + Send + Sync },
            action: $action,
            request: $request,
            response: $response,
            error: $error,
            name: $name,
            payload: $payload,
            response_format: $response_format,
            parse_error: $parse_error,
            $(validate: |$validate_req, $validate_payload, $validate_store| $validate,)?
            handler: $handler,
            span_name: $span_name,
            span_service: $span_service,
            span_attrs: |$span_req, $span_payload, $span_store| $span_attrs,
            authorize: |$auth_req, $auth_payload, $auth_store| $authorize,
        }
    };

    // ------------------------------------------------------------------
    // Simple store + traced handler
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
            traced_handler: $handler:path,
            authorize: |$auth_req:ident, $auth_payload:ident, $auth_store:ident| $authorize:block,
        }
    ) => {
        $crate::impl_aws_action! {
            @traced
            generics: { $store },
            store_type: $store,
            where_clause: { $store: $store_bound + Send + Sync },
            action: $action,
            request: $request,
            response: $response,
            error: $error,
            name: $name,
            payload: $payload,
            response_format: $response_format,
            parse_error: $parse_error,
            $(validate: |$validate_req, $validate_payload, $validate_store| $validate,)?
            handler: $handler,
            authorize: |$auth_req, $auth_payload, $auth_store| $authorize,
        }
    };

    // ------------------------------------------------------------------
    // Composite store + standard handler
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
            span_attrs: |$span_req:ident, $span_payload:ident, $span_store:ident| $span_attrs:block,
            authorize: |$auth_req:ident, $auth_payload:ident, $auth_store:ident| $authorize:block,
        }
    ) => {
        $crate::impl_aws_action! {
            @standard
            generics: { $($store_generic),+ },
            store_type: $store_type,
            where_clause: { $($store_generic: $store_generic_bound + Send + Sync),+ },
            action: $action,
            request: $request,
            response: $response,
            error: $error,
            name: $name,
            payload: $payload,
            response_format: $response_format,
            parse_error: $parse_error,
            $(validate: |$validate_req, $validate_payload, $validate_store| $validate,)?
            handler: $handler,
            span_name: $span_name,
            span_service: $span_service,
            span_attrs: |$span_req, $span_payload, $span_store| $span_attrs,
            authorize: |$auth_req, $auth_payload, $auth_store| $authorize,
        }
    };

    // ------------------------------------------------------------------
    // Composite store + traced handler
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
            traced_handler: $handler:path,
            authorize: |$auth_req:ident, $auth_payload:ident, $auth_store:ident| $authorize:block,
        }
    ) => {
        $crate::impl_aws_action! {
            @traced
            generics: { $($store_generic),+ },
            store_type: $store_type,
            where_clause: { $($store_generic: $store_generic_bound + Send + Sync),+ },
            action: $action,
            request: $request,
            response: $response,
            error: $error,
            name: $name,
            payload: $payload,
            response_format: $response_format,
            parse_error: $parse_error,
            $(validate: |$validate_req, $validate_payload, $validate_store| $validate,)?
            handler: $handler,
            authorize: |$auth_req, $auth_payload, $auth_store| $authorize,
        }
    };

    (
        @standard
        generics: { $($impl_generics:tt)* },
        store_type: $store_type:ty,
        where_clause: { $($where_clause:tt)* },
        action: $action:ident,
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
        span_attrs: |$span_req:ident, $span_payload:ident, $span_store:ident| $span_attrs:block,
        authorize: |$auth_req:ident, $auth_payload:ident, $auth_store:ident| $authorize:block,
    ) => {
        #[$crate::__private::async_trait]
        impl<$($impl_generics)*> $crate::TypedAwsAction<$store_type> for $action
        where
            $($where_clause)*
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
                let $span_req = &request;
                let $span_payload = &payload;
                let $span_store = store;
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

    (
        @traced
        generics: { $($impl_generics:tt)* },
        store_type: $store_type:ty,
        where_clause: { $($where_clause:tt)* },
        action: $action:ident,
        request: $request:ty,
        response: $response:ty,
        error: $error:ty,
        name: $name:literal,
        payload: $payload:ident,
        response_format: $response_format:ident,
        parse_error: $parse_error:path,
        $(validate: |$validate_req:ident, $validate_payload:ident, $validate_store:ident| $validate:block,)?
        handler: $handler:path,
        authorize: |$auth_req:ident, $auth_payload:ident, $auth_store:ident| $authorize:block,
    ) => {
        #[$crate::__private::async_trait]
        impl<$($impl_generics)*> $crate::TypedAwsAction<$store_type> for $action
        where
            $($where_clause)*
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
                $handler(request, payload, store, trace_context, trace_recorder).await
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
