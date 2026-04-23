use async_trait::async_trait;
use hiraeth_core::{
    ApiError, AwsAction, ResolvedRequest, ServiceResponse, auth::AuthorizationCheck,
    parse_aws_query_request, xml_response,
};
use hiraeth_store::{IamStore, iam::Principal};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{
    actions::util::{IAM_XMLNS, IamUserXml, ResponseMetadata, response_metadata, user_arn},
    error::IamError,
};

pub(crate) struct GetUserAction;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct GetUserRequest {
    user_name: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename = "GetUserResponse")]
struct GetUserResponse {
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
impl<S> AwsAction<S> for GetUserAction
where
    S: IamStore + Send + Sync,
{
    fn name(&self) -> &'static str {
        "GetUser"
    }

    async fn handle(
        &self,
        request: ResolvedRequest,
        store: &S,
    ) -> Result<ServiceResponse, ApiError> {
        let get_user_request = match parse_aws_query_request::<GetUserRequest>(&request.request) {
            Ok(request) => request,
            Err(error) => return Ok(ServiceResponse::from(IamError::from(error))),
        };

        let principal =
            match target_user(&request, store, get_user_request.user_name.as_deref()).await {
                Ok(Some(principal)) => principal,
                Ok(None) => {
                    return Ok(ServiceResponse::from(IamError::NoSuchEntity(format!(
                        "User with name '{}' not found",
                        get_user_request.user_name.unwrap_or_else(|| request
                            .auth_context
                            .principal
                            .name
                            .clone())
                    ))));
                }
                Err(error) => return Ok(ServiceResponse::from(IamError::from(error))),
            };

        let user_xml = principal.into();
        match xml_response(&get_user_response(user_xml, Uuid::new_v4().to_string())) {
            Ok(response) => Ok(response),
            Err(error) => Ok(ServiceResponse::from(IamError::from(error))),
        }
    }

    async fn resolve_authorization(
        &self,
        request: &ResolvedRequest,
        store: &S,
    ) -> Result<AuthorizationCheck, ServiceResponse> {
        let get_user_request = parse_aws_query_request::<GetUserRequest>(&request.request)
            .map_err(IamError::from)
            .map_err(ServiceResponse::from)?;
        let principal = target_user(request, store, get_user_request.user_name.as_deref())
            .await
            .map_err(IamError::from)
            .map_err(ServiceResponse::from)?
            .ok_or_else(|| {
                ServiceResponse::from(IamError::NoSuchEntity(format!(
                    "User with name '{}' not found",
                    get_user_request.user_name.unwrap_or_else(|| request
                        .auth_context
                        .principal
                        .name
                        .clone())
                )))
            })?;

        Ok(AuthorizationCheck {
            action: "iam:GetUser".to_string(),
            resource: user_arn(&principal.account_id, &principal.path, &principal.name),
            resource_policy: None,
        })
    }
}

async fn target_user<S>(
    request: &ResolvedRequest,
    store: &S,
    user_name: Option<&str>,
) -> Result<Option<Principal>, hiraeth_store::StoreError>
where
    S: IamStore + Send + Sync,
{
    let name = user_name.unwrap_or(&request.auth_context.principal.name);
    store
        .get_principal_by_identity(&request.auth_context.principal.account_id, "user", name)
        .await
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
    use hiraeth_core::{AuthContext, AwsAction, ResolvedRequest, xml_body};
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
        )
    }

    fn resolved_request(body: &[u8]) -> ResolvedRequest {
        ResolvedRequest {
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
        let action = GetUserAction;
        let response = action
            .handle(
                resolved_request(b"Action=GetUser&Version=2010-05-08&UserName=alice"),
                &store(),
            )
            .await
            .expect("get user should return xml response");

        let body = String::from_utf8(response.body).expect("response body should be utf-8");

        assert_eq!(response.status_code, 200);
        assert!(body.contains("<GetUserResult><User>"));
        assert!(body.contains("<UserName>alice</UserName>"));
        assert!(body.contains("<Path>/engineering/</Path>"));
        assert!(body.contains("<Arn>arn:aws:iam::123456789012:user/engineering/alice</Arn>"));
    }

    #[tokio::test]
    async fn handle_uses_signing_user_when_user_name_is_omitted() {
        let action = GetUserAction;
        let response = action
            .handle(
                resolved_request(b"Action=GetUser&Version=2010-05-08"),
                &store(),
            )
            .await
            .expect("get user should return xml response");

        let body = String::from_utf8(response.body).expect("response body should be utf-8");

        assert_eq!(response.status_code, 200);
        assert!(body.contains("<UserName>signing-user</UserName>"));
        assert!(body.contains("<Arn>arn:aws:iam::123456789012:user/signing-user</Arn>"));
    }

    #[tokio::test]
    async fn resolve_authorization_uses_stored_user_path() {
        let action = GetUserAction;
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
