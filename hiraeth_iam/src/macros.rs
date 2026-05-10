#[macro_export]
macro_rules! impl_iam_action {
    (
        $action:ident < $store:ident : $store_bound:path > {
            request: $request:ty,
            response: $response:ty,
            name: $name:literal,
            $(validate: |$validate_req:ident, $validate_payload:ident, $validate_store:ident| $validate:block,)?
            handler: $handler:path,
            authorize_action: $authorize_action:literal,
        }
    ) => {
        $crate::impl_iam_action! {
            $action<$store: $store_bound> {
                request: $request,
                response: $response,
                name: $name,
                $(validate: |$validate_req, $validate_payload, $validate_store| $validate,)?
                handler: $handler,
                authorize: |request, _payload, store| {
                    $crate::auth::resolve_authorization($authorize_action, request, store).await
                },
            }
        }
    };

    (
        $action:ident < $store:ident : $store_bound:path > {
            request: $request:ty,
            response: $response:ty,
            name: $name:literal,
            $(validate: |$validate_req:ident, $validate_payload:ident, $validate_store:ident| $validate:block,)?
            handler: $handler:path,
            authorize: |$auth_req:ident, $auth_payload:ident, $auth_store:ident| $authorize:block,
        }
    ) => {
        hiraeth_core::impl_aws_action! {
            $action<$store: $store_bound> {
                request: $request,
                response: $response,
                error: $crate::error::IamError,
                name: $name,
                payload: AwsQuery,
                response_format: Xml,
                parse_error: $crate::actions::util::parse_payload_error,
                $(validate: |$validate_req, $validate_payload, $validate_store| $validate,)?
                traced_handler: $handler,
                authorize: |$auth_req, $auth_payload, $auth_store| $authorize,
            }
        }
    };

    (
        $action:ident < $store:ident : $store_bound:path > {
            request: $request:ty,
            response: $response:ty,
            name: $name:literal,
            $(validate: |$validate_req:ident, $validate_payload:ident, $validate_store:ident| $validate:block,)?
            handler: $handler:path,
            span: $span_name:literal,
            span_attrs: |$span_req:ident, $span_payload:ident, $span_store:ident| $span_attrs:block,
            authorize_action: $authorize_action:literal,
        }
    ) => {
        $crate::impl_iam_action! {
            $action<$store: $store_bound> {
                request: $request,
                response: $response,
                name: $name,
                $(validate: |$validate_req, $validate_payload, $validate_store| $validate,)?
                handler: $handler,
                span: $span_name,
                span_attrs: |$span_req, $span_payload, $span_store| $span_attrs,
                authorize: |request, _payload, store| {
                    $crate::auth::resolve_authorization($authorize_action, request, store).await
                },
            }
        }
    };

    (
        $action:ident < $store:ident : $store_bound:path > {
            request: $request:ty,
            response: $response:ty,
            name: $name:literal,
            $(validate: |$validate_req:ident, $validate_payload:ident, $validate_store:ident| $validate:block,)?
            handler: $handler:path,
            span: $span_name:literal,
            span_attrs: |$span_req:ident, $span_payload:ident, $span_store:ident| $span_attrs:block,
            authorize: |$auth_req:ident, $auth_payload:ident, $auth_store:ident| $authorize:block,
        }
    ) => {
        hiraeth_core::impl_aws_action! {
            $action<$store: $store_bound> {
                request: $request,
                response: $response,
                error: $crate::error::IamError,
                name: $name,
                payload: AwsQuery,
                response_format: Xml,
                parse_error: $crate::actions::util::parse_payload_error,
                $(validate: |$validate_req, $validate_payload, $validate_store| $validate,)?
                handler: $handler,
                span_name: $span_name,
                span_service: "iam",
                span_attrs: |$span_req, $span_payload, $span_store| $span_attrs,
                authorize: |$auth_req, $auth_payload, $auth_store| $authorize,
            }
        }
    };
}
