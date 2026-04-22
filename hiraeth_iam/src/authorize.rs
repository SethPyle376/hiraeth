use async_trait::async_trait;
use hiraeth_auth::ResolvedRequest;
use hiraeth_core::{
    ServiceResponse,
    auth::{AuthorizationCheck, PolicyPrincipal},
};
use hiraeth_router::{AuthorizationResult, Authorizer};
use hiraeth_store::IamStore;

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
        let Some(principal) = policy_principal_from_request(request) else {
            return AuthorizationResult::Deny;
        };

        let policy_result = check.resource_policy.as_ref().map(|policy| {
            hiraeth_core::auth::evaluate_policy(&principal, &check.resource, &check.action, policy)
        });

        let authn_result = match policy_result {
            Some(hiraeth_core::auth::PolicyEvalResult::Allowed) => AuthorizationResult::Allow,
            Some(hiraeth_core::auth::PolicyEvalResult::Denied) => AuthorizationResult::Deny,
            _ => AuthorizationResult::Deny, // default to deny if no applicable policy
        };

        match self.mode {
            AuthorizationMode::Enforce => authn_result,
            AuthorizationMode::Audit => {
                tracing::info!(
                    "Audit authz: principal={:?}, resource={}, action={}, result={:?}",
                    principal,
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

    use chrono::{TimeZone, Utc};
    use hiraeth_auth::{AuthContext, ResolvedRequest};
    use hiraeth_core::auth::{AuthorizationCheck, Policy, PolicyPrincipal};
    use hiraeth_http::IncomingRequest;
    use hiraeth_router::{AuthorizationResult, Authorizer};
    use hiraeth_store::iam::{AccessKey, AccessKeyStore, Principal, PrincipalStore};

    use crate::{AuthorizationMode, IamService};

    use super::policy_principal_from_request;

    #[derive(Clone)]
    struct TestIamStore;

    impl AccessKeyStore for TestIamStore {
        async fn get_secret_key(
            &self,
            _access_key: &str,
        ) -> Result<Option<AccessKey>, hiraeth_store::StoreError> {
            Ok(None)
        }

        async fn insert_secret_key(
            &mut self,
            _access_key: &str,
            _secret_key: &str,
            _principal_id: i64,
        ) -> Result<(), hiraeth_store::StoreError> {
            Ok(())
        }
    }

    impl PrincipalStore for TestIamStore {
        async fn get_principal(
            &self,
            _principal_id: i64,
        ) -> Result<Option<Principal>, hiraeth_store::StoreError> {
            Ok(None)
        }
    }

    fn service(mode: AuthorizationMode) -> IamService<TestIamStore> {
        IamService::new(mode, TestIamStore)
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
        let result = service(AuthorizationMode::Enforce)
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
        let result = service(AuthorizationMode::Enforce)
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
        let authorizer = service(AuthorizationMode::Enforce);
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
        let result = service(AuthorizationMode::Audit)
            .authorize(&resolved_request("user"), &check(None))
            .await;

        assert_eq!(result, AuthorizationResult::Allow);
    }

    #[tokio::test]
    async fn off_mode_allows_without_evaluating_policy() {
        let result = service(AuthorizationMode::Off)
            .authorize(&resolved_request("user"), &check(None))
            .await;

        assert_eq!(result, AuthorizationResult::Allow);
    }

    #[tokio::test]
    async fn authorization_denies_when_project_principal_cannot_map_to_policy_principal() {
        let result = service(AuthorizationMode::Enforce)
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
        let response = service(AuthorizationMode::Enforce).unauthorized_response();

        assert_eq!(response.status_code, 403);
        assert!(response.body.is_empty());
    }
}
