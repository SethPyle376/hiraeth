use std::collections::HashMap;

use async_trait::async_trait;
use hiraeth_core::{
    AwsActionPayloadParseError, AwsActionResponseFormat, ResolvedRequest, TypedAwsAction,
    arn_util::policy_arn,
    auth::AuthorizationCheck,
    tracing::{TraceContext, TraceRecorder},
};
use hiraeth_store::{IamStore, iam::ManagedPolicy};
use serde::{Deserialize, Serialize};

use crate::{
    actions::util::{
        IAM_XMLNS, ResponseMetadata, parse_payload_error, requested_or_signing_user,
        response_metadata, validate_user_name,
    },
    error::IamError,
};

pub(crate) struct ListAttachedUserPoliciesAction;

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub(crate) struct ListAttachedUserPoliciesRequest {
    user_name: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "PascalCase")]
pub(crate) struct ListAttachedUserPoliciesResponse {
    #[serde(rename = "@xmlns")]
    xmlns: &'static str,
    list_attached_user_policies_result: ListAttachedUserPoliciesResult,
    response_metadata: ResponseMetadata,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "PascalCase")]
struct ListAttachedUserPoliciesResult {
    attached_policies: AttachedPoliciesXml,
    is_truncated: bool,
}

#[derive(Debug, Clone, Serialize)]
struct AttachedPoliciesXml {
    member: Vec<AttachedPolicyMemberXml>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "PascalCase")]
struct AttachedPolicyMemberXml {
    policy_name: String,
    policy_arn: String,
}

#[async_trait]
impl<S> TypedAwsAction<S> for ListAttachedUserPoliciesAction
where
    S: IamStore + Send + Sync,
{
    type Request = ListAttachedUserPoliciesRequest;
    type Response = ListAttachedUserPoliciesResponse;
    type Error = IamError;

    fn name(&self) -> &'static str {
        "ListAttachedUserPolicies"
    }

    fn parse_error(&self, error: AwsActionPayloadParseError) -> IamError {
        parse_payload_error(error)
    }

    fn response_format(&self) -> AwsActionResponseFormat {
        AwsActionResponseFormat::Xml
    }

    async fn validate(
        &self,
        _request: &ResolvedRequest,
        list_request: &ListAttachedUserPoliciesRequest,
        _store: &S,
    ) -> Result<(), IamError> {
        if let Some(user_name) = &list_request.user_name {
            validate_user_name(user_name)?;
        }

        Ok(())
    }

    async fn handle(
        &self,
        request: ResolvedRequest,
        list_request: ListAttachedUserPoliciesRequest,
        store: &S,
        trace_context: &TraceContext,
        trace_recorder: &dyn TraceRecorder,
    ) -> Result<ListAttachedUserPoliciesResponse, IamError> {
        let requested_user_name = list_request.user_name.clone();
        let target_user =
            requested_or_signing_user(&request, store, list_request.user_name.as_deref()).await?;

        let attributes = HashMap::from([
            (
                "requested_user_name".to_string(),
                requested_user_name.unwrap_or_else(|| "signing_user".to_string()),
            ),
            ("target_user_name".to_string(), target_user.name.clone()),
            ("target_user_id".to_string(), target_user.id.to_string()),
            ("account_id".to_string(), target_user.account_id.clone()),
        ]);

        let policies = trace_context
            .record_result_span(
                trace_recorder,
                "iam.attached_policy.list",
                "iam",
                attributes,
                async {
                    store
                        .get_managed_policies_attached_to_principal(target_user.id)
                        .await
                },
            )
            .await?;

        Ok(list_attached_user_policies_response(
            &target_user.account_id,
            policies,
            request.request_id,
        ))
    }

    async fn resolve_authorization(
        &self,
        request: &ResolvedRequest,
        list_request: ListAttachedUserPoliciesRequest,
        store: &S,
    ) -> Result<AuthorizationCheck, IamError> {
        let target_user =
            requested_or_signing_user(request, store, list_request.user_name.as_deref()).await?;

        Ok(AuthorizationCheck {
            action: "iam:ListAttachedUserPolicies".to_string(),
            resource: hiraeth_core::arn_util::user_arn(
                &target_user.account_id,
                &target_user.path,
                &target_user.name,
            ),
            resource_policy: None,
        })
    }
}

fn list_attached_user_policies_response(
    account_id: &str,
    policies: Vec<ManagedPolicy>,
    request_id: impl Into<String>,
) -> ListAttachedUserPoliciesResponse {
    ListAttachedUserPoliciesResponse {
        xmlns: IAM_XMLNS,
        list_attached_user_policies_result: ListAttachedUserPoliciesResult {
            attached_policies: AttachedPoliciesXml {
                member: policies
                    .into_iter()
                    .map(|policy| attached_policy_member(account_id, policy))
                    .collect(),
            },
            is_truncated: false,
        },
        response_metadata: response_metadata(request_id),
    }
}

fn attached_policy_member(
    account_id: &str,
    policy: ManagedPolicy,
) -> AttachedPolicyMemberXml {
    let policy_path = normalize_policy_path(policy.policy_path.as_deref());
    AttachedPolicyMemberXml {
        policy_name: policy.policy_name.clone(),
        policy_arn: policy_arn(account_id, &policy_path, &policy.policy_name),
    }
}

fn normalize_policy_path(path: Option<&str>) -> String {
    match path {
        Some(path) if !path.trim().is_empty() => {
            let trimmed = path.trim();
            let with_leading = if trimmed.starts_with('/') {
                trimmed.to_string()
            } else {
                format!("/{trimmed}")
            };
            if with_leading.ends_with('/') {
                with_leading
            } else {
                format!("{with_leading}/")
            }
        }
        _ => "/".to_string(),
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
    use hiraeth_store::iam::{
        InMemoryIamStore, ManagedPolicy, ManagedPolicyPrincipalAttachment, Principal,
    };

    use super::{
        ListAttachedUserPoliciesAction, list_attached_user_policies_response,
    };

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

    fn managed_policy(id: i64, name: &str, path: Option<&str>) -> ManagedPolicy {
        ManagedPolicy {
            id,
            policy_id: format!("ANPATESTPOLICY{id:08}"),
            account_id: "123456789012".to_string(),
            policy_name: name.to_string(),
            policy_path: path.map(|p| p.to_string()),
            policy_document: "{}".to_string(),
            created_at: Utc
                .with_ymd_and_hms(2026, 4, 22, 12, 0, 0)
                .unwrap()
                .naive_utc(),
            updated_at: Utc
                .with_ymd_and_hms(2026, 4, 22, 12, 0, 0)
                .unwrap()
                .naive_utc(),
        }
    }

    fn store() -> InMemoryIamStore {
        InMemoryIamStore::new(
            [],
            [
                principal(1, "signing-user", "/"),
                principal(2, "alice", "/engineering/"),
            ],
            [],
            [
                managed_policy(10, "orders-readonly", Some("/dev/")),
                managed_policy(11, "admin-full", None),
                managed_policy(12, "billing-view", Some("/finance/")),
            ],
            [
                ManagedPolicyPrincipalAttachment {
                    id: 1,
                    policy_id: 10,
                    principal_id: 2,
                    created_at: Utc
                        .with_ymd_and_hms(2026, 4, 22, 12, 0, 0)
                        .unwrap()
                        .naive_utc(),
                },
                ManagedPolicyPrincipalAttachment {
                    id: 2,
                    policy_id: 11,
                    principal_id: 2,
                    created_at: Utc
                        .with_ymd_and_hms(2026, 4, 22, 12, 0, 0)
                        .unwrap()
                        .naive_utc(),
                },
            ],
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
                access_key: "AKIA1111111111111111".to_string(),
                principal: principal(1, "signing-user", "/"),
            },
            date: Utc.with_ymd_and_hms(2026, 4, 22, 12, 0, 0).unwrap(),
        }
    }

    #[tokio::test]
    async fn handle_lists_requested_users_attached_policies() {
        let action = TypedAwsActionAdapter::new(ListAttachedUserPoliciesAction);
        let response = action
            .handle(
                resolved_request(
                    b"Action=ListAttachedUserPolicies&Version=2010-05-08&UserName=alice",
                ),
                &store(),
                &TraceContext::new("test-request-id"),
                &NoopTraceRecorder,
            )
            .await;

        let body = String::from_utf8(response.body).expect("response should be utf8");

        assert_eq!(response.status_code, 200);
        assert!(body.contains("<ListAttachedUserPoliciesResponse"));
        assert!(body.contains("<PolicyName>orders-readonly</PolicyName>"));
        assert!(
            body.contains("<PolicyArn>arn:aws:iam::123456789012:policy/dev/orders-readonly</PolicyArn>")
        );
        assert!(body.contains("<PolicyName>admin-full</PolicyName>"));
        assert!(
            body.contains("<PolicyArn>arn:aws:iam::123456789012:policy/admin-full</PolicyArn>")
        );
        assert!(!body.contains("<PolicyName>billing-view</PolicyName>"));
        assert!(body.contains("<IsTruncated>false</IsTruncated>"));
    }

    #[tokio::test]
    async fn handle_uses_signing_user_when_user_name_is_omitted() {
        let action = TypedAwsActionAdapter::new(ListAttachedUserPoliciesAction);
        let response = action
            .handle(
                resolved_request(b"Action=ListAttachedUserPolicies&Version=2010-05-08"),
                &store(),
                &TraceContext::new("test-request-id"),
                &NoopTraceRecorder,
            )
            .await;

        let body = String::from_utf8(response.body).expect("response should be utf8");

        assert_eq!(response.status_code, 200);
        assert!(body.contains("<ListAttachedUserPoliciesResponse"));
        assert!(body.contains("<AttachedPolicies/>") || body.contains("<AttachedPolicies></AttachedPolicies>"));
        assert!(body.contains("<IsTruncated>false</IsTruncated>"));
    }

    #[tokio::test]
    async fn resolve_authorization_uses_target_user_arn() {
        let action = TypedAwsActionAdapter::new(ListAttachedUserPoliciesAction);
        let check = action
            .resolve_authorization(
                &resolved_request(
                    b"Action=ListAttachedUserPolicies&Version=2010-05-08&UserName=alice",
                ),
                &store(),
            )
            .await
            .expect("authorization check should resolve");

        assert_eq!(check.action, "iam:ListAttachedUserPolicies");
        assert_eq!(
            check.resource,
            "arn:aws:iam::123456789012:user/engineering/alice"
        );
    }

    #[test]
    fn list_attached_user_policies_response_serializes_expected_xml_shape() {
        let xml = String::from_utf8(
            xml_body(&list_attached_user_policies_response(
                "123456789012",
                vec![managed_policy(10, "orders-readonly", Some("/dev/"))],
                "request-id",
            ))
            .expect("xml should serialize"),
        )
        .expect("xml should be utf8");

        assert!(xml.contains(
            r#"<ListAttachedUserPoliciesResponse xmlns="https://iam.amazonaws.com/doc/2010-05-08/">"#
        ));
        assert!(xml.contains("<AttachedPolicies><member>"));
        assert!(xml.contains("<PolicyName>orders-readonly</PolicyName>"));
        assert!(xml.contains("<PolicyArn>arn:aws:iam::123456789012:policy/dev/orders-readonly</PolicyArn>"));
        assert!(xml.contains("<ResponseMetadata><RequestId>request-id</RequestId>"));
    }
}
