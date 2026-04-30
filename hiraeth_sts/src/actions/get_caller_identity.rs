use std::collections::HashMap;

use async_trait::async_trait;
use hiraeth_core::{
    AwsActionPayloadParseError, AwsActionResponseFormat, ResolvedRequest, TypedAwsAction, arn_util,
    auth::AuthorizationCheck,
    tracing::{TraceContext, TraceRecorder},
};
use hiraeth_store::IamStore;
use serde::{Deserialize, Serialize};

use crate::{actions::util::parse_payload_error, error::StsError};

pub(crate) struct GetCallerIdentityAction;

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct GetCallerIdentityRequest {}

#[derive(Debug, Serialize)]
#[serde(rename_all = "PascalCase")]
pub(crate) struct GetCallerIdentityResponse {
    #[serde(rename = "@xmlns")]
    xmlns: &'static str,
    #[serde(rename = "GetCallerIdentityResult")]
    result: GetCallerIdentityResult,
    #[serde(rename = "ResponseMetadata")]
    response_metadata: ResponseMetadata,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "PascalCase")]
struct GetCallerIdentityResult {
    arn: String,
    user_id: String,
    account: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "PascalCase")]
struct ResponseMetadata {
    request_id: String,
}

#[async_trait]
impl<S> TypedAwsAction<S> for GetCallerIdentityAction
where
    S: IamStore + Send + Sync,
{
    type Request = GetCallerIdentityRequest;
    type Response = GetCallerIdentityResponse;
    type Error = StsError;

    fn name(&self) -> &'static str {
        "GetCallerIdentity"
    }

    fn parse_error(&self, error: AwsActionPayloadParseError) -> Self::Error {
        parse_payload_error(error)
    }

    fn response_format(&self) -> AwsActionResponseFormat {
        AwsActionResponseFormat::Xml
    }

    async fn handle(
        &self,
        request: ResolvedRequest,
        get_caller_identity_request: Self::Request,
        store: &S,
        trace_context: &TraceContext,
        trace_recorder: &dyn TraceRecorder,
    ) -> Result<GetCallerIdentityResponse, Self::Error> {
        let account_id = &request.auth_context.principal.account_id;
        let name = &request.auth_context.principal.name;

        let user = trace_context
            .record_result_span(
                trace_recorder,
                "sts.identity.lookup",
                "sts",
                HashMap::from([
                    ("account_id".to_string(), account_id.clone()),
                    ("principal_name".to_string(), name.clone()),
                    (
                        "principal_kind".to_string(),
                        request.auth_context.principal.kind.clone(),
                    ),
                ]),
                async {
                    store
                        .get_principal_by_identity(account_id, "user", name)
                        .await
                },
            )
            .await?
            .ok_or_else(|| StsError::InternalError("User not found".to_string()))?;

        let response = GetCallerIdentityResponse {
            xmlns: "https://sts.amazonaws.com/doc/2011-06-15/",
            result: GetCallerIdentityResult {
                arn: arn_util::user_arn(account_id, &user.path, &user.name),
                user_id: user.user_id.clone(),
                account: account_id.clone(),
            },
            response_metadata: ResponseMetadata {
                request_id: request.request_id,
            },
        };

        Ok(response)
    }

    async fn resolve_authorization_typed(
        &self,
        request: &ResolvedRequest,
        get_caller_identity_request: GetCallerIdentityRequest,
        store: &S,
    ) -> Result<AuthorizationCheck, StsError> {
        Ok(AuthorizationCheck {
            action: "sts:GetCallerIdentity".to_string(),
            resource: format!(
                "arn:aws:iam::{}:user/{}",
                request.auth_context.principal.account_id, request.auth_context.principal.name
            ),
            resource_policy: None,
        })
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use chrono::{TimeZone, Utc};
    use hiraeth_core::{
        AuthContext, AwsAction, ResolvedRequest, TypedAwsActionAdapter,
        tracing::{NoopTraceRecorder, TraceContext},
    };
    use hiraeth_http::IncomingRequest;
    use hiraeth_store::iam::{AccessKey, InMemoryIamStore, Principal};

    use super::GetCallerIdentityAction;

    fn principal(id: i64, name: &str, path: &str) -> Principal {
        Principal {
            id,
            account_id: "123456789012".to_string(),
            kind: "user".to_string(),
            name: name.to_string(),
            path: path.to_string(),
            user_id: format!("AIDATESTUSER{id:08}"),
            created_at: Utc
                .with_ymd_and_hms(2026, 4, 28, 12, 0, 0)
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
                    .with_ymd_and_hms(2026, 4, 28, 12, 0, 0)
                    .unwrap()
                    .naive_utc(),
            }],
            [principal(1, "signing-user", "/engineering/")],
            [],
            [],
            [],
        )
    }

    fn resolved_request(body: &[u8]) -> ResolvedRequest {
        ResolvedRequest {
            request_id: "test-request-id".to_string(),
            request: IncomingRequest {
                host: "sts.amazonaws.com".to_string(),
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
            service: "sts".to_string(),
            region: "us-east-1".to_string(),
            auth_context: AuthContext {
                access_key: "AKIAIOSFODNN7EXAMPLE".to_string(),
                principal: principal(1, "signing-user", "/engineering/"),
            },
            date: Utc.with_ymd_and_hms(2026, 4, 28, 12, 0, 0).unwrap(),
        }
    }

    #[tokio::test]
    async fn handle_returns_expected_identity_fields() {
        let action = TypedAwsActionAdapter::new(GetCallerIdentityAction);
        let response = action
            .handle(
                resolved_request(b"Action=GetCallerIdentity&Version=2011-06-15"),
                &store(),
                &TraceContext::new("test-request-id"),
                &NoopTraceRecorder,
            )
            .await;

        assert_eq!(response.status_code, 200);
        let body = String::from_utf8(response.body).expect("response should be utf8");
        assert!(body.contains("<GetCallerIdentityResponse"));
        assert!(body.contains("<Account>123456789012</Account>"));
        assert!(body.contains("<UserId>AIDATESTUSER00000001</UserId>"));
        assert!(
            body.contains("<Arn>arn:aws:iam::123456789012:user/engineering/signing-user</Arn>")
        );
        assert!(body.contains("<ResponseMetadata><RequestId>test-request-id</RequestId>"));
    }

    #[tokio::test]
    async fn resolve_authorization_uses_sts_action_name() {
        let action = TypedAwsActionAdapter::new(GetCallerIdentityAction);
        let check = action
            .resolve_authorization(
                &resolved_request(b"Action=GetCallerIdentity&Version=2011-06-15"),
                &store(),
            )
            .await
            .expect("auth check should resolve");

        assert_eq!(check.action, "sts:GetCallerIdentity");
        assert_eq!(
            check.resource,
            "arn:aws:iam::123456789012:user/signing-user"
        );
    }
}
