use std::collections::HashMap;

use async_trait::async_trait;
use hiraeth_core::{
    AwsActionPayloadParseError, AwsActionResponseFormat, ResolvedRequest, TypedAwsAction, arn_util,
    auth::AuthorizationCheck,
    tracing::{TraceContext, TraceRecorder},
};
use hiraeth_store::IamStore;
use serde::{Deserialize, Serialize};

use crate::{
    actions::util::{
        IAM_XMLNS, IamUserXml, ResponseMetadata, optional_target_user, parse_payload_error,
        response_metadata,
    },
    error::IamError,
};

pub(crate) struct GetUserAction;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub(crate) struct GetUserRequest {
    user_name: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename = "GetUserResponse")]
pub(crate) struct GetUserResponse {
    #[serde(rename = "@xmlns")]
    xmlns: &'static str,
    #[serde(rename = "GetUserResult")]
    result: GetUserResult,
    #[serde(rename = "ResponseMetadata")]
    response_metadata: ResponseMetadata,
}

#[derive(Debug, Serialize)]
struct GetUserResult {
    #[serde(rename = "User")]
    user: IamUserXml,
}

#[async_trait]
impl<S> TypedAwsAction<S> for GetUserAction
where
    S: IamStore + Send + Sync,
{
    type Request = GetUserRequest;
    type Response = GetUserResponse;
    type Error = IamError;

    fn name(&self) -> &'static str {
        "GetUser"
    }

    fn parse_error(&self, error: AwsActionPayloadParseError) -> IamError {
        parse_payload_error(error)
    }

    fn response_format(&self) -> AwsActionResponseFormat {
        AwsActionResponseFormat::Xml
    }

    async fn handle(
        &self,
        request: ResolvedRequest,
        get_user_request: GetUserRequest,
        store: &S,
        trace_context: &TraceContext,
        trace_recorder: &dyn TraceRecorder,
    ) -> Result<GetUserResponse, IamError> {
        let timer = trace_context.start_span();
        let requested_user_name = get_user_request.user_name.clone();
        let principal =
            match optional_target_user(&request, store, get_user_request.user_name.as_deref())
                .await?
            {
                Some(principal) => principal,
                None => {
                    return Err(IamError::NoSuchEntity(format!(
                        "User with name '{}' not found",
                        get_user_request.user_name.unwrap_or_else(|| request
                            .auth_context
                            .principal
                            .name
                            .clone())
                    )));
                }
            };
        let attributes = HashMap::from([
            (
                "requested_user_name".to_string(),
                requested_user_name.unwrap_or_else(|| "signing_user".to_string()),
            ),
            ("user_name".to_string(), principal.name.clone()),
            ("user_id".to_string(), principal.id.to_string()),
            ("account_id".to_string(), principal.account_id.clone()),
            ("path".to_string(), principal.path.clone()),
        ]);
        trace_context
            .record_span_or_warn(
                trace_recorder,
                timer,
                "iam.user.lookup",
                "iam",
                "ok",
                attributes,
            )
            .await;

        Ok(get_user_response(principal.into(), request.request_id))
    }

    async fn resolve_authorization_typed(
        &self,
        request: &ResolvedRequest,
        get_user_request: GetUserRequest,
        store: &S,
    ) -> Result<AuthorizationCheck, IamError> {
        let principal = optional_target_user(request, store, get_user_request.user_name.as_deref())
            .await?
            .ok_or_else(|| {
                IamError::NoSuchEntity(format!(
                    "User with name '{}' not found",
                    get_user_request.user_name.unwrap_or_else(|| request
                        .auth_context
                        .principal
                        .name
                        .clone())
                ))
            })?;

        Ok(AuthorizationCheck {
            action: "iam:GetUser".to_string(),
            resource: arn_util::user_arn(&principal.account_id, &principal.path, &principal.name),
            resource_policy: None,
        })
    }
}

fn get_user_response(user: IamUserXml, request_id: impl Into<String>) -> GetUserResponse {
    GetUserResponse {
        xmlns: IAM_XMLNS,
        result: GetUserResult { user },
        response_metadata: response_metadata(request_id),
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use chrono::{TimeZone, Utc};
    use hiraeth_core::{
        AuthContext, AwsAction, ResolvedRequest, TypedAwsActionAdapter,
        tracing::{NoopTraceRecorder, TraceContext},
        xml_body,
    };
    use hiraeth_http::IncomingRequest;
    use hiraeth_store::iam::{AccessKey, InMemoryIamStore, Principal};

    use super::{GetUserAction, IamUserXml, get_user_response};

    fn principal(id: i64, name: &str, path: &str) -> Principal {
        Principal {
            id,
            account_id: "123456789012".to_string(),
            kind: "user".to_string(),
            name: name.to_string(),
            path: path.to_string(),
            user_id: format!("AIDATESTUSER{id:08}"),
            created_at: Utc
                .with_ymd_and_hms(2026, 4, 22, 12, 0, 0)
                .unwrap()
                .naive_utc(),
        }
    }

    fn store() -> InMemoryIamStore {
        InMemoryIamStore::new(
            [AccessKey {
                key_id: "AKIAIOSFODNN7EXAMPLE".to_string(),
                principal_id: 1,
                secret_key: "secret".to_string(),
                created_at: Utc
                    .with_ymd_and_hms(2026, 4, 22, 12, 0, 0)
                    .unwrap()
                    .naive_utc(),
            }],
            [
                principal(1, "signing-user", "/"),
                principal(2, "alice", "/engineering/"),
            ],
            [],
            [],
            [],
        )
    }

    fn resolved_request(body: &[u8]) -> ResolvedRequest {
        ResolvedRequest {
            request_id: "test-request-id".to_string(),
            request: IncomingRequest {
                host: "iam.amazonaws.com".to_string(),
                method: "POST".to_string(),
                path: "/".to_string(),
                query: None,
                headers: [(
                    "content-type".to_string(),
                    "application/x-www-form-urlencoded".to_string(),
                )]
                .into_iter()
                .collect::<HashMap<_, _>>(),
                body: body.to_vec(),
            },
            service: "iam".to_string(),
            region: "us-east-1".to_string(),
            auth_context: AuthContext {
                access_key: "AKIAIOSFODNN7EXAMPLE".to_string(),
                principal: principal(1, "signing-user", "/"),
            },
            date: Utc.with_ymd_and_hms(2026, 4, 22, 12, 0, 0).unwrap(),
        }
    }

    #[tokio::test]
    async fn handle_returns_nested_user_xml_for_requested_user() {
        let action = TypedAwsActionAdapter::new(GetUserAction);
        let response = action
            .handle(
                resolved_request(b"Action=GetUser&Version=2010-05-08&UserName=alice"),
                &store(),
                &TraceContext::new("test-request-id"),
                &NoopTraceRecorder,
            )
            .await;

        let body = String::from_utf8(response.body).expect("response body should be utf-8");

        assert_eq!(response.status_code, 200);
        assert!(body.contains("<GetUserResult><User>"));
        assert!(body.contains("<UserName>alice</UserName>"));
        assert!(body.contains("<Path>/engineering/</Path>"));
        assert!(body.contains("<Arn>arn:aws:iam::123456789012:user/engineering/alice</Arn>"));
    }

    #[tokio::test]
    async fn handle_uses_signing_user_when_user_name_is_omitted() {
        let action = TypedAwsActionAdapter::new(GetUserAction);
        let response = action
            .handle(
                resolved_request(b"Action=GetUser&Version=2010-05-08"),
                &store(),
                &TraceContext::new("test-request-id"),
                &NoopTraceRecorder,
            )
            .await;

        let body = String::from_utf8(response.body).expect("response body should be utf-8");

        assert_eq!(response.status_code, 200);
        assert!(body.contains("<UserName>signing-user</UserName>"));
        assert!(body.contains("<Arn>arn:aws:iam::123456789012:user/signing-user</Arn>"));
    }

    #[tokio::test]
    async fn resolve_authorization_uses_stored_user_path() {
        let action = TypedAwsActionAdapter::new(GetUserAction);
        let check = action
            .resolve_authorization(
                &resolved_request(b"Action=GetUser&Version=2010-05-08&UserName=alice"),
                &store(),
            )
            .await
            .expect("auth check should resolve");

        assert_eq!(check.action, "iam:GetUser");
        assert_eq!(
            check.resource,
            "arn:aws:iam::123456789012:user/engineering/alice"
        );
        assert!(check.resource_policy.is_none());
    }

    #[test]
    fn get_user_response_serializes_expected_xml_shape() {
        let xml = xml_body(&get_user_response(
            IamUserXml {
                path: "/division_abc/subdivision_xyz/".to_string(),
                user_name: "Bob".to_string(),
                user_id: "AIDACKCEVSQ6C2EXAMPLE".to_string(),
                arn: "arn:aws:iam::123456789012:user/division_abc/subdivision_xyz/Bob".to_string(),
                create_date: "2026-04-23T18:20:17Z".to_string(),
            },
            "7a62c49f-347e-4fc4-9331-6e8eEXAMPLE",
        ))
        .expect("get user response should serialize");

        assert_eq!(
            String::from_utf8(xml).unwrap(),
            concat!(
                r#"<GetUserResponse xmlns="https://iam.amazonaws.com/doc/2010-05-08/">"#,
                r#"<GetUserResult><User>"#,
                r#"<Path>/division_abc/subdivision_xyz/</Path>"#,
                r#"<UserName>Bob</UserName>"#,
                r#"<UserId>AIDACKCEVSQ6C2EXAMPLE</UserId>"#,
                r#"<Arn>arn:aws:iam::123456789012:user/division_abc/subdivision_xyz/Bob</Arn>"#,
                r#"<CreateDate>2026-04-23T18:20:17Z</CreateDate>"#,
                r#"</User></GetUserResult>"#,
                r#"<ResponseMetadata>"#,
                r#"<RequestId>7a62c49f-347e-4fc4-9331-6e8eEXAMPLE</RequestId>"#,
                r#"</ResponseMetadata></GetUserResponse>"#
            )
        );
    }
}
