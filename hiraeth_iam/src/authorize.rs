use async_trait::async_trait;
use hiraeth_core::{
    AuthContext, ResolvedRequest, ServiceResponse,
    auth::{
        AuthorizationCheck, Policy, PolicyEvalResult, PolicyPrincipal, evaluate_identity_policy,
        evaluate_resource_policy,
    },
};
use hiraeth_router::{AuthorizationResult, Authorizer};
use hiraeth_store::{IamStore, iam::PrincipalInlinePolicyStore};

use crate::{AuthorizationMode, IamService};

#[async_trait]
impl<S> Authorizer for IamService<S>
where
    S: IamStore + Send + Sync,
{
    async fn authorize(
        &self,
        request: &ResolvedRequest,
        check: &AuthorizationCheck,
    ) -> AuthorizationResult {
        let resource_principal = policy_principal_from_request(request);
        let identity_policy_result = match evaluate_principal_inline_policies(
            &self.store,
            request.auth_context.principal.id,
            &check.resource,
            &check.action,
        )
        .await
        {
            Ok(result) => result,
            Err(error) => {
                tracing::error!(
                    principal_id = request.auth_context.principal.id,
                    resource = %check.resource,
                    action = %check.action,
                    "failed to evaluate inline policies: {error}"
                );
                PolicyEvalResult::Denied
            }
        };
        let resource_policy_result = match (&resource_principal, check.resource_policy.as_ref()) {
            (Some(principal), Some(policy)) => {
                evaluate_resource_policy(principal, &check.resource, &check.action, policy)
            }
            _ => PolicyEvalResult::NotApplicable,
        };
        let policy_result =
            combine_policy_results([identity_policy_result, resource_policy_result]);

        let authn_result = match policy_result {
            PolicyEvalResult::Allowed => AuthorizationResult::Allow,
            PolicyEvalResult::Denied | PolicyEvalResult::NotApplicable => AuthorizationResult::Deny,
        };

        match self.mode {
            AuthorizationMode::Enforce => authn_result,
            AuthorizationMode::Audit => {
                tracing::info!(
                    "Audit authz: principal={:?}, resource={}, action={}, result={:?}",
                    resource_principal.as_ref(),
                    check.resource,
                    check.action,
                    authn_result
                );
                AuthorizationResult::Allow // allow the request but log the result
            }
            AuthorizationMode::Off => AuthorizationResult::Allow, // allow all requests
        }
    }

    fn unauthorized_response(&self) -> ServiceResponse {
        ServiceResponse {
            status_code: 403,
            body: vec![],
            headers: vec![],
        }
    }
}

fn combine_policy_results(results: impl IntoIterator<Item = PolicyEvalResult>) -> PolicyEvalResult {
    results
        .into_iter()
        .fold(PolicyEvalResult::NotApplicable, |acc, result| {
            match (acc, result) {
                (PolicyEvalResult::Denied, _) => PolicyEvalResult::Denied,
                (_, PolicyEvalResult::Denied) => PolicyEvalResult::Denied,
                (PolicyEvalResult::Allowed, _) => PolicyEvalResult::Allowed,
                (_, PolicyEvalResult::Allowed) => PolicyEvalResult::Allowed,
                _ => PolicyEvalResult::NotApplicable,
            }
        })
}

async fn evaluate_principal_inline_policies<S: IamStore + Send + Sync>(
    store: &S,
    principal_id: i64,
    resource: &str,
    action: &str,
) -> Result<PolicyEvalResult, String> {
    let policies = store
        .get_inline_policies_for_principal(principal_id)
        .await
        .map_err(|error| error.to_string())?;

    let policy_results = policies
        .into_iter()
        .map(|policy| {
            serde_json::from_str::<Policy>(&policy.policy_document)
                .map(|policy_document| evaluate_identity_policy(resource, action, &policy_document))
                .map_err(|error| {
                    format!(
                        "invalid policy document for inline policy {}: {}",
                        policy.policy_name, error
                    )
                })
        })
        .collect::<Result<Vec<_>, _>>()?;

    Ok(combine_policy_results(policy_results))
}

fn policy_principal_from_request(request: &ResolvedRequest) -> Option<PolicyPrincipal> {
    let request_principal = request.auth_context.principal.clone();
    match request_principal.kind.as_str() {
        "user" => Some(PolicyPrincipal::User {
            account_id: request_principal.account_id.clone(),
            user_name: request_principal.name.clone(),
        }),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use async_trait::async_trait;
    use chrono::{TimeZone, Utc};
    use hiraeth_core::{
        AuthContext, ResolvedRequest,
        auth::{AuthorizationCheck, Policy, PolicyPrincipal},
    };
    use hiraeth_http::IncomingRequest;
    use hiraeth_router::{AuthorizationResult, Authorizer};
    use hiraeth_store::iam::{
        AccessKey, AccessKeyStore, NewPrincipal, Principal, PrincipalInlinePolicy,
        PrincipalInlinePolicyStore, PrincipalStore,
    };

    use crate::{AuthorizationMode, IamService};

    use super::policy_principal_from_request;

    #[derive(Clone, Default)]
    struct TestIamStore {
        inline_policies: Vec<PrincipalInlinePolicy>,
    }

    #[async_trait]
    impl AccessKeyStore for TestIamStore {
        async fn get_secret_key(
            &self,
            _access_key: &str,
        ) -> Result<Option<AccessKey>, hiraeth_store::StoreError> {
            Ok(None)
        }

        async fn list_access_keys_for_principal(
            &self,
            _principal_id: i64,
        ) -> Result<Vec<AccessKey>, hiraeth_store::StoreError> {
            Ok(vec![])
        }

        async fn insert_secret_key(
            &self,
            _access_key: &str,
            _secret_key: &str,
            _principal_id: i64,
        ) -> Result<AccessKey, hiraeth_store::StoreError> {
            unreachable!("authorization tests do not insert access keys")
        }
    }

    #[async_trait]
    impl PrincipalStore for TestIamStore {
        async fn get_principal(
            &self,
            _principal_id: i64,
        ) -> Result<Option<Principal>, hiraeth_store::StoreError> {
            Ok(None)
        }

        async fn get_principal_by_identity(
            &self,
            _account_id: &str,
            _kind: &str,
            _name: &str,
        ) -> Result<Option<Principal>, hiraeth_store::StoreError> {
            Ok(None)
        }

        async fn list_principals(&self) -> Result<Vec<Principal>, hiraeth_store::StoreError> {
            Ok(vec![])
        }

        async fn create_principal(
            &self,
            principal: NewPrincipal,
        ) -> Result<Principal, hiraeth_store::StoreError> {
            Ok(Principal {
                id: 999,
                account_id: principal.account_id,
                kind: principal.kind,
                name: principal.name,
                path: principal.path,
                user_id: principal.user_id,
                created_at: Utc::now().naive_utc(),
            })
        }

        async fn delete_user(
            &self,
            _account_id: &str,
            _name: &str,
        ) -> Result<(), hiraeth_store::StoreError> {
            unreachable!("authorization tests do not delete users")
        }
    }

    #[async_trait]
    impl PrincipalInlinePolicyStore for TestIamStore {
        async fn get_inline_policies_for_principal(
            &self,
            principal_id: i64,
        ) -> Result<Vec<PrincipalInlinePolicy>, hiraeth_store::StoreError> {
            Ok(self
                .inline_policies
                .iter()
                .filter(|policy| policy.principal_id == principal_id)
                .cloned()
                .collect())
        }
    }

    fn service(
        mode: AuthorizationMode,
        inline_policies: impl IntoIterator<Item = PrincipalInlinePolicy>,
    ) -> IamService<TestIamStore> {
        IamService::new(
            mode,
            TestIamStore {
                inline_policies: inline_policies.into_iter().collect(),
            },
        )
    }

    fn resolved_request(kind: &str) -> ResolvedRequest {
        ResolvedRequest {
            request: IncomingRequest {
                host: "sqs.us-east-1.amazonaws.com".to_string(),
                method: "POST".to_string(),
                path: "/".to_string(),
                query: None,
                headers: HashMap::new(),
                body: Vec::new(),
            },
            service: "sqs".to_string(),
            region: "us-east-1".to_string(),
            auth_context: AuthContext {
                access_key: "AKIAIOSFODNN7EXAMPLE".to_string(),
                principal: Principal {
                    id: 1,
                    account_id: "123456789012".to_string(),
                    kind: kind.to_string(),
                    name: "alice".to_string(),
                    path: "/".to_string(),
                    user_id: "AIDATESTUSER000001".to_string(),
                    created_at: Utc
                        .with_ymd_and_hms(2026, 4, 20, 12, 0, 0)
                        .unwrap()
                        .naive_utc(),
                },
            },
            date: Utc.with_ymd_and_hms(2026, 4, 20, 12, 0, 0).unwrap(),
        }
    }

    fn user_principal() -> PolicyPrincipal {
        PolicyPrincipal::User {
            account_id: "123456789012".to_string(),
            user_name: "alice".to_string(),
        }
    }

    fn policy(effect: &str, action: &str, resource: &str) -> Policy {
        serde_json::from_str(&format!(
            r#"{{
                "Version":"2012-10-17",
                "Statement":[
                    {{
                        "Effect":"{effect}",
                        "Principal":{{"AWS":"arn:aws:iam::123456789012:user/alice"}},
                        "Action":"{action}",
                        "Resource":"{resource}"
                    }}
                ]
            }}"#
        ))
        .expect("test policy should deserialize")
    }

    fn check(resource_policy: Option<Policy>) -> AuthorizationCheck {
        AuthorizationCheck {
            action: "sqs:SendMessage".to_string(),
            resource: "arn:aws:sqs:us-east-1:123456789012:orders".to_string(),
            resource_policy,
        }
    }

    fn auth_check(
        action: &str,
        resource: &str,
        resource_policy: Option<Policy>,
    ) -> AuthorizationCheck {
        AuthorizationCheck {
            action: action.to_string(),
            resource: resource.to_string(),
            resource_policy,
        }
    }

    fn inline_policy(
        effect: &str,
        action: &str,
        resource: &str,
        principal_id: i64,
    ) -> PrincipalInlinePolicy {
        PrincipalInlinePolicy {
            id: 1,
            principal_id,
            policy_name: format!("{}-{}", effect.to_lowercase(), action),
            policy_document: format!(
                r#"{{
                    "Version":"2012-10-17",
                    "Statement":[
                        {{
                            "Effect":"{effect}",
                            "Action":"{action}",
                            "Resource":"{resource}"
                        }}
                    ]
                }}"#
            ),
            created_at: Utc
                .with_ymd_and_hms(2026, 4, 20, 12, 0, 0)
                .unwrap()
                .naive_utc(),
            updated_at: Utc
                .with_ymd_and_hms(2026, 4, 20, 12, 0, 0)
                .unwrap()
                .naive_utc(),
        }
    }

    #[tokio::test]
    async fn get_principal_maps_project_user_to_policy_user() {
        let principal = policy_principal_from_request(&resolved_request("user"));

        assert_eq!(principal, Some(user_principal()));
    }

    #[tokio::test]
    async fn get_principal_returns_none_for_unknown_project_principal_kind() {
        let principal = policy_principal_from_request(&resolved_request("unknown"));

        assert_eq!(principal, None);
    }

    #[tokio::test]
    async fn enforce_mode_allows_matching_resource_policy() {
        let result = service(AuthorizationMode::Enforce, [])
            .authorize(
                &resolved_request("user"),
                &check(Some(policy(
                    "Allow",
                    "sqs:SendMessage",
                    "arn:aws:sqs:us-east-1:123456789012:orders",
                ))),
            )
            .await;

        assert_eq!(result, AuthorizationResult::Allow);
    }

    #[tokio::test]
    async fn enforce_mode_denies_matching_deny_policy() {
        let result = service(AuthorizationMode::Enforce, [])
            .authorize(
                &resolved_request("user"),
                &check(Some(policy(
                    "Deny",
                    "sqs:SendMessage",
                    "arn:aws:sqs:us-east-1:123456789012:orders",
                ))),
            )
            .await;

        assert_eq!(result, AuthorizationResult::Deny);
    }

    #[tokio::test]
    async fn enforce_mode_denies_missing_or_not_applicable_resource_policy() {
        let authorizer = service(AuthorizationMode::Enforce, []);
        let no_policy_result = authorizer
            .authorize(&resolved_request("user"), &check(None))
            .await;
        let not_applicable_result = authorizer
            .authorize(
                &resolved_request("user"),
                &check(Some(policy(
                    "Allow",
                    "sqs:ReceiveMessage",
                    "arn:aws:sqs:us-east-1:123456789012:orders",
                ))),
            )
            .await;

        assert_eq!(no_policy_result, AuthorizationResult::Deny);
        assert_eq!(not_applicable_result, AuthorizationResult::Deny);
    }

    #[tokio::test]
    async fn audit_mode_allows_even_when_policy_would_deny() {
        let result = service(AuthorizationMode::Audit, [])
            .authorize(&resolved_request("user"), &check(None))
            .await;

        assert_eq!(result, AuthorizationResult::Allow);
    }

    #[tokio::test]
    async fn off_mode_allows_without_evaluating_policy() {
        let result = service(AuthorizationMode::Off, [])
            .authorize(&resolved_request("user"), &check(None))
            .await;

        assert_eq!(result, AuthorizationResult::Allow);
    }

    #[tokio::test]
    async fn authorization_denies_when_project_principal_cannot_map_to_policy_principal() {
        let result = service(AuthorizationMode::Enforce, [])
            .authorize(
                &resolved_request("unknown"),
                &check(Some(policy(
                    "Allow",
                    "sqs:SendMessage",
                    "arn:aws:sqs:us-east-1:123456789012:orders",
                ))),
            )
            .await;

        assert_eq!(result, AuthorizationResult::Deny);
    }

    #[test]
    fn unauthorized_response_is_forbidden() {
        let response = service(AuthorizationMode::Enforce, []).unauthorized_response();

        assert_eq!(response.status_code, 403);
        assert!(response.body.is_empty());
    }

    #[tokio::test]
    async fn enforce_mode_allows_matching_inline_identity_policy_without_resource_policy() {
        let result = service(
            AuthorizationMode::Enforce,
            [inline_policy(
                "Allow",
                "sqs:SendMessage",
                "arn:aws:sqs:us-east-1:123456789012:orders",
                1,
            )],
        )
        .authorize(&resolved_request("user"), &check(None))
        .await;

        assert_eq!(result, AuthorizationResult::Allow);
    }

    #[tokio::test]
    async fn enforce_mode_denies_when_inline_identity_policy_denies_resource_allowed_by_resource_policy()
     {
        let result = service(
            AuthorizationMode::Enforce,
            [inline_policy(
                "Deny",
                "sqs:SendMessage",
                "arn:aws:sqs:us-east-1:123456789012:orders",
                1,
            )],
        )
        .authorize(
            &resolved_request("user"),
            &check(Some(policy(
                "Allow",
                "sqs:SendMessage",
                "arn:aws:sqs:us-east-1:123456789012:orders",
            ))),
        )
        .await;

        assert_eq!(result, AuthorizationResult::Deny);
    }

    #[tokio::test]
    async fn enforce_mode_allows_inline_identity_policy_even_without_resource_principal_mapping() {
        let result = service(
            AuthorizationMode::Enforce,
            [inline_policy(
                "Allow",
                "sqs:SendMessage",
                "arn:aws:sqs:us-east-1:123456789012:orders",
                1,
            )],
        )
        .authorize(&resolved_request("unknown"), &check(None))
        .await;

        assert_eq!(result, AuthorizationResult::Allow);
    }

    #[tokio::test]
    async fn enforce_mode_allows_seeded_admin_style_inline_policy_for_send_message() {
        let result = service(
            AuthorizationMode::Enforce,
            [inline_policy("Allow", "*", "arn:aws:*:*:123456789012:*", 1)],
        )
        .authorize(&resolved_request("user"), &check(None))
        .await;

        assert_eq!(result, AuthorizationResult::Allow);
    }

    #[tokio::test]
    async fn enforce_mode_allows_seeded_admin_style_inline_policy_for_create_queue() {
        let result = service(
            AuthorizationMode::Enforce,
            [inline_policy("Allow", "*", "arn:aws:*:*:123456789012:*", 1)],
        )
        .authorize(
            &resolved_request("user"),
            &auth_check(
                "sqs:CreateQueue",
                "arn:aws:sqs:us-east-1:123456789012:*",
                None,
            ),
        )
        .await;

        assert_eq!(result, AuthorizationResult::Allow);
    }
}
