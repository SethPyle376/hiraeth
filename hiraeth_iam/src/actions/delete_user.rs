use async_trait::async_trait;
use hiraeth_core::{
    ApiError, AwsActionPayloadFormat, AwsActionPayloadParseError, ResolvedRequest, ServiceResponse,
    TypedAwsAction, auth::AuthorizationCheck, xml_response,
};
use hiraeth_store::IamStore;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{
    actions::util::{IAM_XMLNS, ResponseMetadata, parse_payload_error, user_arn},
    error::IamError,
};

pub(crate) struct DeleteUserAction;

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub(crate) struct DeleteUserRequest {
    pub user_name: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename = "DeleteUserResponse")]
#[serde(rename_all = "PascalCase")]
struct DeleteUserResponse {
    #[serde(rename = "@xmlns")]
    xmlns: &'static str,
    #[serde(rename = "ResponseMetadata")]
    response_metadata: ResponseMetadata,
}

fn delete_user_response(request_id: impl Into<String>) -> DeleteUserResponse {
    DeleteUserResponse {
        xmlns: IAM_XMLNS,
        response_metadata: ResponseMetadata {
            request_id: request_id.into(),
        },
    }
}

#[async_trait]
impl<S> TypedAwsAction<S> for DeleteUserAction
where
    S: IamStore + Send + Sync,
{
    type Request = DeleteUserRequest;

    fn name(&self) -> &'static str {
        "DeleteUser"
    }

    fn payload_format(&self) -> AwsActionPayloadFormat {
        AwsActionPayloadFormat::AwsQuery
    }

    fn parse_error(&self, error: AwsActionPayloadParseError) -> ServiceResponse {
        parse_payload_error(error)
    }

    async fn handle_typed(
        &self,
        request: ResolvedRequest,
        delete_request: DeleteUserRequest,
        store: &S,
    ) -> Result<ServiceResponse, ApiError> {
        let account_id = &request.auth_context.principal.account_id;

        let result = store
            .delete_user(account_id, &delete_request.user_name)
            .await
            .map_err(|e| ServiceResponse::from(IamError::from(e)));

        match result {
            Ok(_) => {
                match xml_response(&delete_user_response(Uuid::new_v4().to_string()))
                    .map_err(IamError::from)
                {
                    Ok(response) => Ok(response),
                    Err(error) => Ok(ServiceResponse::from(error)),
                }
            }
            Err(e) => Ok(e),
        }
    }

    async fn resolve_authorization_typed(
        &self,
        request: &ResolvedRequest,
        delete_user_request: DeleteUserRequest,
        store: &S,
    ) -> Result<AuthorizationCheck, ServiceResponse> {
        let principal = store
            .get_principal_by_identity(
                &request.auth_context.principal.account_id,
                "user",
                &delete_user_request.user_name,
            )
            .await
            .map_err(|e| ServiceResponse::from(IamError::from(e)))?
            .ok_or_else(|| {
                ServiceResponse::from(IamError::NoSuchEntity(format!(
                    "User with name {} does not exist",
                    delete_user_request.user_name
                )))
            })?;

        Ok(AuthorizationCheck {
            action: "iam:DeleteUser".to_string(),
            resource: user_arn(
                &request.auth_context.principal.account_id,
                &principal.path,
                &principal.name,
            ),
            resource_policy: None,
        })
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use chrono::{TimeZone, Utc};
    use hiraeth_core::{AuthContext, AwsAction, ResolvedRequest, TypedAwsActionAdapter, xml_body};
    use hiraeth_http::IncomingRequest;
    use hiraeth_store::iam::{AccessKey, InMemoryIamStore, Principal, PrincipalStore};

    use super::{DeleteUserAction, delete_user_response};

    fn principal(id: i64, name: &str, path: &str) -> Principal {
        Principal {
            id,
            account_id: "123456789012".to_string(),
            kind: "user".to_string(),
            name: name.to_string(),
            path: path.to_string(),
            user_id: format!("AIDATESTUSER{id:08}"),
            created_at: Utc
                .with_ymd_and_hms(2026, 4, 24, 12, 0, 0)
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
                    .with_ymd_and_hms(2026, 4, 24, 12, 0, 0)
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
            date: Utc.with_ymd_and_hms(2026, 4, 24, 12, 0, 0).unwrap(),
        }
    }

    #[tokio::test]
    async fn handle_deletes_requested_user_and_returns_xml_metadata() {
        let action = TypedAwsActionAdapter::new(DeleteUserAction);
        let store = store();
        let response = action
            .handle(
                resolved_request(b"Action=DeleteUser&Version=2010-05-08&UserName=alice"),
                &store,
            )
            .await
            .expect("delete user should return xml response");

        let body = String::from_utf8(response.body).expect("response body should be utf-8");

        assert_eq!(response.status_code, 200);
        assert_eq!(
            response.headers,
            vec![(
                "content-type".to_string(),
                "text/xml; charset=utf-8".to_string()
            )]
        );
        assert!(
            body.contains(
                r#"<DeleteUserResponse xmlns="https://iam.amazonaws.com/doc/2010-05-08/">"#
            )
        );
        assert!(body.contains("<ResponseMetadata><RequestId>"));
        assert!(
            store
                .get_principal_by_identity("123456789012", "user", "alice")
                .await
                .expect("principal lookup should succeed")
                .is_none()
        );
        assert!(
            store
                .get_principal_by_identity("123456789012", "user", "signing-user")
                .await
                .expect("principal lookup should succeed")
                .is_some()
        );
    }

    #[tokio::test]
    async fn handle_returns_no_such_entity_for_missing_user() {
        let action = TypedAwsActionAdapter::new(DeleteUserAction);
        let response = action
            .handle(
                resolved_request(b"Action=DeleteUser&Version=2010-05-08&UserName=missing"),
                &store(),
            )
            .await
            .expect("delete user should render service response");

        assert_eq!(response.status_code, 404);
        assert!(
            String::from_utf8(response.body)
                .unwrap()
                .contains("not found")
        );
    }

    #[tokio::test]
    async fn resolve_authorization_uses_stored_user_path() {
        let action = TypedAwsActionAdapter::new(DeleteUserAction);
        let check = action
            .resolve_authorization(
                &resolved_request(b"Action=DeleteUser&Version=2010-05-08&UserName=alice"),
                &store(),
            )
            .await
            .expect("auth check should resolve");

        assert_eq!(check.action, "iam:DeleteUser");
        assert_eq!(
            check.resource,
            "arn:aws:iam::123456789012:user/engineering/alice"
        );
        assert!(check.resource_policy.is_none());
    }

    #[test]
    fn delete_user_response_serializes_expected_xml_shape() {
        let xml = xml_body(&delete_user_response("7a62c49f-347e-4fc4-9331-6e8eEXAMPLE"))
            .expect("delete user response should serialize");

        assert_eq!(
            String::from_utf8(xml).unwrap(),
            concat!(
                r#"<DeleteUserResponse xmlns="https://iam.amazonaws.com/doc/2010-05-08/">"#,
                r#"<ResponseMetadata>"#,
                r#"<RequestId>7a62c49f-347e-4fc4-9331-6e8eEXAMPLE</RequestId>"#,
                r#"</ResponseMetadata></DeleteUserResponse>"#
            )
        );
    }
}
