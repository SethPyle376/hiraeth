use crate::auth::{Policy, policy::PolicyStatement, principal::PolicyPrincipal};
use wildmatch::WildMatch;

#[derive(Debug, PartialEq, Eq)]
pub enum PolicyEvalResult {
    Allowed,
    Denied,
    NotApplicable,
}

pub fn evaluate_resource_policy(
    principal: &PolicyPrincipal,
    resource: &str,
    action: &str,
    policy: &Policy,
) -> PolicyEvalResult {
    evaluate_matching_statements(policy, |statement| {
        let principal_matches = !statement.principal.is_empty()
            && statement
                .principal
                .iter()
                .any(|pattern| principal_matches_pattern(pattern, principal));
        let action_matches = statement
            .action
            .iter()
            .any(|pattern| wildcard_match(pattern, action));
        let resource_matches = statement
            .resource
            .iter()
            .any(|pattern| wildcard_match(pattern, resource));

        principal_matches && action_matches && resource_matches
    })
}

pub fn evaluate_identity_policy(resource: &str, action: &str, policy: &Policy) -> PolicyEvalResult {
    evaluate_matching_statements(policy, |statement| {
        let principal_matches = statement.principal.is_empty();
        let action_matches = statement
            .action
            .iter()
            .any(|pattern| wildcard_match(pattern, action));
        let resource_matches = statement
            .resource
            .iter()
            .any(|pattern| wildcard_match(pattern, resource));

        principal_matches && action_matches && resource_matches
    })
}

fn evaluate_matching_statements(
    policy: &Policy,
    mut matches_statement: impl Fn(&PolicyStatement) -> bool,
) -> PolicyEvalResult {
    let statement_results = policy
        .statement
        .iter()
        .map(|statement| evaluate_statement(statement, &mut matches_statement));

    statement_results.fold(PolicyEvalResult::NotApplicable, |acc, result| {
        match (acc, result) {
            (PolicyEvalResult::Denied, _) => PolicyEvalResult::Denied, // Deny overrides all
            (_, PolicyEvalResult::Denied) => PolicyEvalResult::Denied,
            (PolicyEvalResult::Allowed, _) => PolicyEvalResult::Allowed, // Allow if no Deny
            (_, PolicyEvalResult::Allowed) => PolicyEvalResult::Allowed,
            _ => PolicyEvalResult::NotApplicable,
        }
    })
}

fn evaluate_statement(
    statement: &PolicyStatement,
    matches_statement: &mut impl Fn(&PolicyStatement) -> bool,
) -> PolicyEvalResult {
    if matches_statement(statement) {
        match statement.effect.as_str() {
            "Allow" => PolicyEvalResult::Allowed,
            "Deny" => PolicyEvalResult::Denied,
            _ => PolicyEvalResult::NotApplicable,
        }
    } else {
        PolicyEvalResult::NotApplicable
    }
}

fn principal_matches_pattern(pattern: &PolicyPrincipal, principal: &PolicyPrincipal) -> bool {
    match pattern {
        PolicyPrincipal::Any => true,
        PolicyPrincipal::Account(account_pattern) => principal_account_id(principal)
            .is_some_and(|account_id| wildcard_match(account_pattern, account_id)),
        PolicyPrincipal::User {
            account_id,
            user_name,
        } => match principal {
            PolicyPrincipal::User {
                account_id: principal_account_id,
                user_name: principal_user_name,
            } => {
                wildcard_match(account_id, principal_account_id)
                    && wildcard_match(user_name, principal_user_name)
            }
            _ => false,
        },
        PolicyPrincipal::Role {
            account_id,
            role_name,
        } => match principal {
            PolicyPrincipal::Role {
                account_id: principal_account_id,
                role_name: principal_role_name,
            } => {
                wildcard_match(account_id, principal_account_id)
                    && wildcard_match(role_name, principal_role_name)
            }
            _ => false,
        },
        PolicyPrincipal::AssumedRole {
            account_id,
            role_name,
            session_name,
        } => match principal {
            PolicyPrincipal::AssumedRole {
                account_id: principal_account_id,
                role_name: principal_role_name,
                session_name: principal_session_name,
            } => {
                wildcard_match(account_id, principal_account_id)
                    && wildcard_match(role_name, principal_role_name)
                    && wildcard_match(session_name, principal_session_name)
            }
            _ => false,
        },
        PolicyPrincipal::Service(service_pattern) => match principal {
            PolicyPrincipal::Service(service_name) => wildcard_match(service_pattern, service_name),
            _ => false,
        },
    }
}

fn principal_account_id(principal: &PolicyPrincipal) -> Option<&str> {
    match principal {
        PolicyPrincipal::Any => None,
        PolicyPrincipal::Account(account_id)
        | PolicyPrincipal::User { account_id, .. }
        | PolicyPrincipal::Role { account_id, .. }
        | PolicyPrincipal::AssumedRole { account_id, .. } => Some(account_id),
        PolicyPrincipal::Service(_) => None,
    }
}

fn wildcard_match(pattern: &str, value: &str) -> bool {
    WildMatch::new(pattern).matches(value)
}

#[cfg(test)]
mod tests {
    use crate::auth::{
        Policy, PolicyEvalResult, PolicyPrincipal, evaluate_identity_policy,
        evaluate_resource_policy,
    };

    use super::PolicyStatement;

    fn user_principal() -> PolicyPrincipal {
        PolicyPrincipal::User {
            account_id: "123456789012".to_string(),
            user_name: "alice".to_string(),
        }
    }

    fn statement(effect: &str, action: &str, resource: &str) -> PolicyStatement {
        PolicyStatement {
            effect: effect.to_string(),
            principal: vec![user_principal()],
            action: vec![action.to_string()],
            resource: vec![resource.to_string()],
        }
    }

    fn identity_statement(effect: &str, action: &str, resource: &str) -> PolicyStatement {
        PolicyStatement {
            effect: effect.to_string(),
            principal: Vec::new(),
            action: vec![action.to_string()],
            resource: vec![resource.to_string()],
        }
    }

    fn policy(statement: Vec<PolicyStatement>) -> Policy {
        Policy {
            version: "2012-10-17".to_string(),
            statement,
        }
    }

    #[test]
    fn evaluate_policy_allows_exact_matching_statement() {
        let policy = policy(vec![statement(
            "Allow",
            "sqs:SendMessage",
            "arn:aws:sqs:us-east-1:123456789012:orders",
        )]);

        let result = evaluate_resource_policy(
            &user_principal(),
            "arn:aws:sqs:us-east-1:123456789012:orders",
            "sqs:SendMessage",
            &policy,
        );

        assert_eq!(result, PolicyEvalResult::Allowed);
    }

    #[test]
    fn evaluate_policy_denies_exact_matching_statement() {
        let policy = policy(vec![statement(
            "Deny",
            "sqs:SendMessage",
            "arn:aws:sqs:us-east-1:123456789012:orders",
        )]);

        let result = evaluate_resource_policy(
            &user_principal(),
            "arn:aws:sqs:us-east-1:123456789012:orders",
            "sqs:SendMessage",
            &policy,
        );

        assert_eq!(result, PolicyEvalResult::Denied);
    }

    #[test]
    fn evaluate_policy_denies_when_any_matching_statement_denies() {
        let policy = policy(vec![
            statement(
                "Allow",
                "sqs:SendMessage",
                "arn:aws:sqs:us-east-1:123456789012:orders",
            ),
            statement(
                "Deny",
                "sqs:SendMessage",
                "arn:aws:sqs:us-east-1:123456789012:orders",
            ),
        ]);

        let result = evaluate_resource_policy(
            &user_principal(),
            "arn:aws:sqs:us-east-1:123456789012:orders",
            "sqs:SendMessage",
            &policy,
        );

        assert_eq!(result, PolicyEvalResult::Denied);
    }

    #[test]
    fn evaluate_policy_returns_not_applicable_when_no_statement_matches() {
        let policy = policy(vec![statement(
            "Allow",
            "sqs:SendMessage",
            "arn:aws:sqs:us-east-1:123456789012:orders",
        )]);

        let result = evaluate_resource_policy(
            &user_principal(),
            "arn:aws:sqs:us-east-1:123456789012:orders",
            "sqs:ReceiveMessage",
            &policy,
        );

        assert_eq!(result, PolicyEvalResult::NotApplicable);
    }

    #[test]
    fn evaluate_policy_matches_wildcard_action_pattern() {
        let policy = policy(vec![statement(
            "Allow",
            "sqs:*",
            "arn:aws:sqs:us-east-1:123456789012:orders",
        )]);

        let result = evaluate_resource_policy(
            &user_principal(),
            "arn:aws:sqs:us-east-1:123456789012:orders",
            "sqs:DeleteMessage",
            &policy,
        );

        assert_eq!(result, PolicyEvalResult::Allowed);
    }

    #[test]
    fn evaluate_policy_matches_single_character_action_pattern() {
        let policy = policy(vec![statement(
            "Allow",
            "sqs:SendMessag?",
            "arn:aws:sqs:us-east-1:123456789012:orders",
        )]);

        let result = evaluate_resource_policy(
            &user_principal(),
            "arn:aws:sqs:us-east-1:123456789012:orders",
            "sqs:SendMessage",
            &policy,
        );

        assert_eq!(result, PolicyEvalResult::Allowed);
    }

    #[test]
    fn evaluate_policy_matches_multiple_wildcards_in_action_pattern() {
        let policy = policy(vec![statement(
            "Allow",
            "sqs:*Mess*ge*",
            "arn:aws:sqs:us-east-1:123456789012:orders",
        )]);

        let result = evaluate_resource_policy(
            &user_principal(),
            "arn:aws:sqs:us-east-1:123456789012:orders",
            "sqs:SendMessage",
            &policy,
        );

        assert_eq!(result, PolicyEvalResult::Allowed);
    }

    #[test]
    fn evaluate_policy_matches_wildcard_resource_pattern() {
        let policy = policy(vec![statement(
            "Allow",
            "sqs:SendMessage",
            "arn:aws:sqs:us-east-1:123456789012:*",
        )]);

        let result = evaluate_resource_policy(
            &user_principal(),
            "arn:aws:sqs:us-east-1:123456789012:orders",
            "sqs:SendMessage",
            &policy,
        );

        assert_eq!(result, PolicyEvalResult::Allowed);
    }

    #[test]
    fn evaluate_policy_matches_single_character_resource_pattern() {
        let policy = policy(vec![statement(
            "Allow",
            "sqs:SendMessage",
            "arn:aws:sqs:us-east-1:123456789012:order?",
        )]);

        let result = evaluate_resource_policy(
            &user_principal(),
            "arn:aws:sqs:us-east-1:123456789012:orders",
            "sqs:SendMessage",
            &policy,
        );

        assert_eq!(result, PolicyEvalResult::Allowed);
    }

    #[test]
    fn evaluate_policy_matches_multiple_wildcards_in_resource_pattern() {
        let policy = policy(vec![statement(
            "Allow",
            "sqs:SendMessage",
            "arn:aws:sqs:*:1234*9012:*ord*",
        )]);

        let result = evaluate_resource_policy(
            &user_principal(),
            "arn:aws:sqs:us-east-1:123456789012:orders",
            "sqs:SendMessage",
            &policy,
        );

        assert_eq!(result, PolicyEvalResult::Allowed);
    }

    #[test]
    fn evaluate_policy_matches_wildcard_user_principal_pattern() {
        let policy = policy(vec![PolicyStatement {
            effect: "Allow".to_string(),
            principal: vec![PolicyPrincipal::User {
                account_id: "123456789012".to_string(),
                user_name: "*".to_string(),
            }],
            action: vec!["sqs:SendMessage".to_string()],
            resource: vec!["arn:aws:sqs:us-east-1:123456789012:orders".to_string()],
        }]);

        let result = evaluate_resource_policy(
            &user_principal(),
            "arn:aws:sqs:us-east-1:123456789012:orders",
            "sqs:SendMessage",
            &policy,
        );

        assert_eq!(result, PolicyEvalResult::Allowed);
    }

    #[test]
    fn evaluate_policy_matches_single_character_user_principal_pattern() {
        let policy = policy(vec![PolicyStatement {
            effect: "Allow".to_string(),
            principal: vec![PolicyPrincipal::User {
                account_id: "123456789012".to_string(),
                user_name: "alic?".to_string(),
            }],
            action: vec!["sqs:SendMessage".to_string()],
            resource: vec!["arn:aws:sqs:us-east-1:123456789012:orders".to_string()],
        }]);

        let result = evaluate_resource_policy(
            &user_principal(),
            "arn:aws:sqs:us-east-1:123456789012:orders",
            "sqs:SendMessage",
            &policy,
        );

        assert_eq!(result, PolicyEvalResult::Allowed);
    }

    #[test]
    fn evaluate_policy_matches_multiple_wildcards_in_user_principal_pattern() {
        let policy = policy(vec![PolicyStatement {
            effect: "Allow".to_string(),
            principal: vec![PolicyPrincipal::User {
                account_id: "1234*9012".to_string(),
                user_name: "a*i?e".to_string(),
            }],
            action: vec!["sqs:SendMessage".to_string()],
            resource: vec!["arn:aws:sqs:us-east-1:123456789012:orders".to_string()],
        }]);

        let result = evaluate_resource_policy(
            &user_principal(),
            "arn:aws:sqs:us-east-1:123456789012:orders",
            "sqs:SendMessage",
            &policy,
        );

        assert_eq!(result, PolicyEvalResult::Allowed);
    }

    #[test]
    fn evaluate_policy_matches_account_principal_against_user_in_same_account() {
        let policy = policy(vec![PolicyStatement {
            effect: "Allow".to_string(),
            principal: vec![PolicyPrincipal::Account("123456789012".to_string())],
            action: vec!["sqs:SendMessage".to_string()],
            resource: vec!["arn:aws:sqs:us-east-1:123456789012:orders".to_string()],
        }]);

        let result = evaluate_resource_policy(
            &user_principal(),
            "arn:aws:sqs:us-east-1:123456789012:orders",
            "sqs:SendMessage",
            &policy,
        );

        assert_eq!(result, PolicyEvalResult::Allowed);
    }

    #[test]
    fn evaluate_policy_matches_single_character_account_principal_pattern() {
        let policy = policy(vec![PolicyStatement {
            effect: "Allow".to_string(),
            principal: vec![PolicyPrincipal::Account("12345678901?".to_string())],
            action: vec!["sqs:SendMessage".to_string()],
            resource: vec!["arn:aws:sqs:us-east-1:123456789012:orders".to_string()],
        }]);

        let result = evaluate_resource_policy(
            &user_principal(),
            "arn:aws:sqs:us-east-1:123456789012:orders",
            "sqs:SendMessage",
            &policy,
        );

        assert_eq!(result, PolicyEvalResult::Allowed);
    }

    #[test]
    fn evaluate_policy_matches_any_principal_and_global_wildcards() {
        let policy = policy(vec![PolicyStatement {
            effect: "Allow".to_string(),
            principal: vec![PolicyPrincipal::Any],
            action: vec!["*".to_string()],
            resource: vec!["*".to_string()],
        }]);

        let result = evaluate_resource_policy(
            &user_principal(),
            "arn:aws:sqs:us-east-1:123456789012:orders",
            "sqs:SendMessage",
            &policy,
        );

        assert_eq!(result, PolicyEvalResult::Allowed);
    }

    #[test]
    fn evaluate_policy_matches_service_principal_with_multiple_wildcards() {
        let policy = policy(vec![PolicyStatement {
            effect: "Allow".to_string(),
            principal: vec![PolicyPrincipal::Service("s*.*amazonaws.com".to_string())],
            action: vec!["sqs:SendMessage".to_string()],
            resource: vec!["arn:aws:sqs:us-east-1:123456789012:orders".to_string()],
        }]);

        let result = evaluate_resource_policy(
            &PolicyPrincipal::Service("sns.amazonaws.com".to_string()),
            "arn:aws:sqs:us-east-1:123456789012:orders",
            "sqs:SendMessage",
            &policy,
        );

        assert_eq!(result, PolicyEvalResult::Allowed);
    }

    #[test]
    fn evaluate_policy_denies_when_wildcard_deny_overrides_wildcard_allow() {
        let policy = policy(vec![
            PolicyStatement {
                effect: "Allow".to_string(),
                principal: vec![PolicyPrincipal::User {
                    account_id: "123456789012".to_string(),
                    user_name: "a*".to_string(),
                }],
                action: vec!["sqs:*".to_string()],
                resource: vec!["arn:aws:sqs:us-east-1:123456789012:*".to_string()],
            },
            PolicyStatement {
                effect: "Deny".to_string(),
                principal: vec![PolicyPrincipal::User {
                    account_id: "123456789012".to_string(),
                    user_name: "*".to_string(),
                }],
                action: vec!["sqs:*Delete*".to_string()],
                resource: vec!["arn:aws:sqs:*:123456789012:order*".to_string()],
            },
        ]);

        let result = evaluate_resource_policy(
            &user_principal(),
            "arn:aws:sqs:us-east-1:123456789012:orders",
            "sqs:DeleteMessage",
            &policy,
        );

        assert_eq!(result, PolicyEvalResult::Denied);
    }

    #[test]
    fn evaluate_policy_matches_wildcard_pattern_across_slashes() {
        let policy = policy(vec![PolicyStatement {
            effect: "Allow".to_string(),
            principal: vec![PolicyPrincipal::Role {
                account_id: "123456789012".to_string(),
                role_name: "service/*".to_string(),
            }],
            action: vec!["sqs:SendMessage".to_string()],
            resource: vec!["arn:aws:sqs:us-east-1:123456789012:orders".to_string()],
        }]);

        let result = evaluate_resource_policy(
            &PolicyPrincipal::Role {
                account_id: "123456789012".to_string(),
                role_name: "service/team/app-worker".to_string(),
            },
            "arn:aws:sqs:us-east-1:123456789012:orders",
            "sqs:SendMessage",
            &policy,
        );

        assert_eq!(result, PolicyEvalResult::Allowed);
    }

    #[test]
    fn evaluate_identity_policy_allows_principalless_statement() {
        let policy = policy(vec![identity_statement(
            "Allow",
            "sqs:SendMessage",
            "arn:aws:sqs:us-east-1:123456789012:orders",
        )]);

        let result = evaluate_identity_policy(
            "arn:aws:sqs:us-east-1:123456789012:orders",
            "sqs:SendMessage",
            &policy,
        );

        assert_eq!(result, PolicyEvalResult::Allowed);
    }

    #[test]
    fn evaluate_resource_policy_ignores_principalless_statement() {
        let policy = policy(vec![identity_statement(
            "Allow",
            "sqs:SendMessage",
            "arn:aws:sqs:us-east-1:123456789012:orders",
        )]);

        let result = evaluate_resource_policy(
            &user_principal(),
            "arn:aws:sqs:us-east-1:123456789012:orders",
            "sqs:SendMessage",
            &policy,
        );

        assert_eq!(result, PolicyEvalResult::NotApplicable);
    }

    #[test]
    fn evaluate_identity_policy_ignores_statement_with_principal() {
        let policy = policy(vec![statement(
            "Allow",
            "sqs:SendMessage",
            "arn:aws:sqs:us-east-1:123456789012:orders",
        )]);

        let result = evaluate_identity_policy(
            "arn:aws:sqs:us-east-1:123456789012:orders",
            "sqs:SendMessage",
            &policy,
        );

        assert_eq!(result, PolicyEvalResult::NotApplicable);
    }

    #[test]
    fn evaluate_identity_policy_denies_when_matching_statement_denies() {
        let policy = policy(vec![
            identity_statement("Allow", "sqs:*", "arn:aws:sqs:us-east-1:123456789012:*"),
            identity_statement(
                "Deny",
                "sqs:DeleteMessage",
                "arn:aws:sqs:us-east-1:123456789012:orders",
            ),
        ]);

        let result = evaluate_identity_policy(
            "arn:aws:sqs:us-east-1:123456789012:orders",
            "sqs:DeleteMessage",
            &policy,
        );

        assert_eq!(result, PolicyEvalResult::Denied);
    }
}
