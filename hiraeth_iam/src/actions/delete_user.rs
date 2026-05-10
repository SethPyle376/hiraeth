use std::collections::HashMap;

use hiraeth_core::{
    ResolvedRequest,
    tracing::{TraceContext, TraceRecorder},
};
use hiraeth_store::IamStore;
use serde::{Deserialize, Serialize};

use crate::{
    actions::util::{IAM_XMLNS, ResponseMetadata, response_metadata, validate_user_name},
    error::IamError,
};

pub(crate) struct DeleteUserAction;

crate::impl_iam_action! {
    DeleteUserAction<S: IamStore> {
        request: DeleteUserRequest,
        response: DeleteUserResponse,
        name: "DeleteUser",
        validate: |_request, delete_request, _store| {
            validate_user_name(&delete_request.user_name)
        },
        handler: handle_delete_user,
        authorize_action: "iam:DeleteUser",
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub(crate) struct DeleteUserRequest {
    pub user_name: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename = "DeleteUserResponse")]
#[serde(rename_all = "PascalCase")]
pub(crate) struct DeleteUserResponse {
    #[serde(rename = "@xmlns")]
    xmlns: &'static str,
    #[serde(rename = "ResponseMetadata")]
    response_metadata: ResponseMetadata,
}

async fn handle_delete_user<S: IamStore + Send + Sync>(
    request: ResolvedRequest,
    delete_request: DeleteUserRequest,
    store: &S,
    trace_context: &TraceContext,
    trace_recorder: &dyn TraceRecorder,
) -> Result<DeleteUserResponse, IamError> {
    let account_id = &request.auth_context.principal.account_id;
    let attributes = HashMap::from([
        ("account_id".to_string(), account_id.clone()),
        ("user_name".to_string(), delete_request.user_name.clone()),
    ]);

    trace_context
        .record_result_span(
            trace_recorder,
            "iam.user.delete",
            "iam",
            attributes,
            async {
                store
                    .delete_user(account_id, &delete_request.user_name)
                    .await
            },
        )
        .await
        .map(|_| delete_user_response(request.request_id))
        .map_err(Into::into)
}

fn delete_user_response(request_id: impl Into<String>) -> DeleteUserResponse {
    DeleteUserResponse {
        xmlns: IAM_XMLNS,
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
                &TraceContext::new("test-request-id"),
                &NoopTraceRecorder,
            )
            .await;

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
                &TraceContext::new("test-request-id"),
                &NoopTraceRecorder,
            )
            .await;

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
