/// Implements [`TypedAwsAction`] for a unit-struct action.
///
/// Service crates provide an [`AwsActionDefaults`] implementation for the
/// protocol defaults they share, while individual actions provide only the
/// behavior that differs: request/response models, validation, handler,
/// tracing attributes, and authorization.
#[macro_export]
macro_rules! impl_aws_action {
    (
        $action:ident < $store:ident : $store_bound:path > {
            request: $request:ty,
            response: $response:ty,
            defaults: $defaults:path,
            name: $name:literal,
            $(response_format: $response_format:ident,)?
            $(validate: |$validate_req:ident, $validate_payload:ident, $validate_store:ident| $validate:block,)?
            handler: $handler:path,
            span: $span_name:literal,
            span_attrs: |$span_req:ident, $span_payload:ident, $span_store:ident| $span_attrs:block,
            authorize_action: $authorize_action:literal,
            authorize_with: $authorize_with:path,
        }
    ) => {
        $crate::impl_aws_action! {
            $action<$store: $store_bound> {
                request: $request,
                response: $response,
                defaults: $defaults,
                name: $name,
                $(response_format: $response_format,)?
                $(validate: |$validate_req, $validate_payload, $validate_store| $validate,)?
                handler: $handler,
                span: $span_name,
                span_attrs: |$span_req, $span_payload, $span_store| $span_attrs,
                authorize: |request, _payload, store| {
                    $authorize_with($authorize_action, request, store).await
                },
            }
        }
    };

    (
        $action:ident < $store:ident : $store_bound:path > {
            request: $request:ty,
            response: $response:ty,
            defaults: $defaults:path,
            name: $name:literal,
            $(response_format: $response_format:ident,)?
            $(validate: |$validate_req:ident, $validate_payload:ident, $validate_store:ident| $validate:block,)?
            handler: $handler:path,
            span: $span_name:literal,
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
            defaults: $defaults,
            name: $name,
            response_format: { $($response_format)? },
            $(validate: |$validate_req, $validate_payload, $validate_store| $validate,)?
            handler: $handler,
            span_name: $span_name,
            span_attrs: |$span_req, $span_payload, $span_store| $span_attrs,
            authorize: |$auth_req, $auth_payload, $auth_store| $authorize,
        }
    };

    (
        $action:ident < $store:ident : $store_bound:path > {
            request: $request:ty,
            response: $response:ty,
            defaults: $defaults:path,
            name: $name:literal,
            $(response_format: $response_format:ident,)?
            $(validate: |$validate_req:ident, $validate_payload:ident, $validate_store:ident| $validate:block,)?
            handler: $handler:path,
            authorize_action: $authorize_action:literal,
            authorize_with: $authorize_with:path,
        }
    ) => {
        $crate::impl_aws_action! {
            $action<$store: $store_bound> {
                request: $request,
                response: $response,
                defaults: $defaults,
                name: $name,
                $(response_format: $response_format,)?
                $(validate: |$validate_req, $validate_payload, $validate_store| $validate,)?
                handler: $handler,
                authorize: |request, _payload, store| {
                    $authorize_with($authorize_action, request, store).await
                },
            }
        }
    };

    (
        $action:ident < $store:ident : $store_bound:path > {
            request: $request:ty,
            response: $response:ty,
            defaults: $defaults:path,
            name: $name:literal,
            $(response_format: $response_format:ident,)?
            $(validate: |$validate_req:ident, $validate_payload:ident, $validate_store:ident| $validate:block,)?
            handler: $handler:path,
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
            defaults: $defaults,
            name: $name,
            response_format: { $($response_format)? },
            $(validate: |$validate_req, $validate_payload, $validate_store| $validate,)?
            handler: $handler,
            authorize: |$auth_req, $auth_payload, $auth_store| $authorize,
        }
    };

    (
        $action:ident < $store_type:ty > where $($store_generic:ident : $store_generic_bound:path),+ $(,)? {
            request: $request:ty,
            response: $response:ty,
            defaults: $defaults:path,
            name: $name:literal,
            $(response_format: $response_format:ident,)?
            $(validate: |$validate_req:ident, $validate_payload:ident, $validate_store:ident| $validate:block,)?
            handler: $handler:path,
            span: $span_name:literal,
            span_attrs: |$span_req:ident, $span_payload:ident, $span_store:ident| $span_attrs:block,
            authorize_action: $authorize_action:literal,
            authorize_with: $authorize_with:path,
        }
    ) => {
        $crate::impl_aws_action! {
            $action<$store_type> where $($store_generic: $store_generic_bound),+ {
                request: $request,
                response: $response,
                defaults: $defaults,
                name: $name,
                $(response_format: $response_format,)?
                $(validate: |$validate_req, $validate_payload, $validate_store| $validate,)?
                handler: $handler,
                span: $span_name,
                span_attrs: |$span_req, $span_payload, $span_store| $span_attrs,
                authorize: |request, _payload, store| {
                    $authorize_with($authorize_action, request, store).await
                },
            }
        }
    };

    (
        $action:ident < $store_type:ty > where $($store_generic:ident : $store_generic_bound:path),+ $(,)? {
            request: $request:ty,
            response: $response:ty,
            defaults: $defaults:path,
            name: $name:literal,
            $(response_format: $response_format:ident,)?
            $(validate: |$validate_req:ident, $validate_payload:ident, $validate_store:ident| $validate:block,)?
            handler: $handler:path,
            span: $span_name:literal,
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
            defaults: $defaults,
            name: $name,
            response_format: { $($response_format)? },
            $(validate: |$validate_req, $validate_payload, $validate_store| $validate,)?
            handler: $handler,
            span_name: $span_name,
            span_attrs: |$span_req, $span_payload, $span_store| $span_attrs,
            authorize: |$auth_req, $auth_payload, $auth_store| $authorize,
        }
    };

    (
        $action:ident < $store_type:ty > where $($store_generic:ident : $store_generic_bound:path),+ $(,)? {
            request: $request:ty,
            response: $response:ty,
            defaults: $defaults:path,
            name: $name:literal,
            $(response_format: $response_format:ident,)?
            $(validate: |$validate_req:ident, $validate_payload:ident, $validate_store:ident| $validate:block,)?
            handler: $handler:path,
            authorize_action: $authorize_action:literal,
            authorize_with: $authorize_with:path,
        }
    ) => {
        $crate::impl_aws_action! {
            $action<$store_type> where $($store_generic: $store_generic_bound),+ {
                request: $request,
                response: $response,
                defaults: $defaults,
                name: $name,
                $(response_format: $response_format,)?
                $(validate: |$validate_req, $validate_payload, $validate_store| $validate,)?
                handler: $handler,
                authorize: |request, _payload, store| {
                    $authorize_with($authorize_action, request, store).await
                },
            }
        }
    };

    (
        $action:ident < $store_type:ty > where $($store_generic:ident : $store_generic_bound:path),+ $(,)? {
            request: $request:ty,
            response: $response:ty,
            defaults: $defaults:path,
            name: $name:literal,
            $(response_format: $response_format:ident,)?
            $(validate: |$validate_req:ident, $validate_payload:ident, $validate_store:ident| $validate:block,)?
            handler: $handler:path,
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
            defaults: $defaults,
            name: $name,
            response_format: { $($response_format)? },
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
        defaults: $defaults:path,
        name: $name:literal,
        response_format: { $($response_format:ident)? },
        $(validate: |$validate_req:ident, $validate_payload:ident, $validate_store:ident| $validate:block,)?
        handler: $handler:path,
        span_name: $span_name:literal,
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
            type Error = <$defaults as $crate::AwsActionDefaults>::Error;

            fn name(&self) -> &'static str {
                $name
            }

            fn payload_format(&self) -> $crate::AwsActionPayloadFormat {
                <$defaults as $crate::AwsActionDefaults>::PAYLOAD_FORMAT
            }

            fn response_format(&self) -> $crate::AwsActionResponseFormat {
                $crate::impl_aws_action!(@response_format $defaults; $($response_format)?)
            }

            fn parse_error(&self, error: $crate::AwsActionPayloadParseError) -> Self::Error {
                <$defaults as $crate::AwsActionDefaults>::parse_error(error)
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
                        <$defaults as $crate::AwsActionDefaults>::SPAN_SERVICE,
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
        defaults: $defaults:path,
        name: $name:literal,
        response_format: { $($response_format:ident)? },
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
            type Error = <$defaults as $crate::AwsActionDefaults>::Error;

            fn name(&self) -> &'static str {
                $name
            }

            fn payload_format(&self) -> $crate::AwsActionPayloadFormat {
                <$defaults as $crate::AwsActionDefaults>::PAYLOAD_FORMAT
            }

            fn response_format(&self) -> $crate::AwsActionResponseFormat {
                $crate::impl_aws_action!(@response_format $defaults; $($response_format)?)
            }

            fn parse_error(&self, error: $crate::AwsActionPayloadParseError) -> Self::Error {
                <$defaults as $crate::AwsActionDefaults>::parse_error(error)
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

    (@response_format $defaults:path;) => {
        <$defaults as $crate::AwsActionDefaults>::RESPONSE_FORMAT
    };

    (@response_format $defaults:path; $response_format:ident) => {
        $crate::AwsActionResponseFormat::$response_format
    };
}
