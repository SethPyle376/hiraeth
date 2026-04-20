use crate::auth::{Policy, policy::PolicyStatement, principal::PolicyPrincipal};

#[derive(Debug, PartialEq, Eq)]
pub enum PolicyEvalResult {
    Allowed,
    Denied,
    NotApplicable,
}

pub fn evaluate_policy(
    principal: &PolicyPrincipal,
    resource: &str,
    action: &str,
    policy: &Policy,
) -> PolicyEvalResult {
    let statement_results = policy
        .statement
        .iter()
        .map(|statement| evaluate_statement(principal, resource, action, statement));

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
    principal: &PolicyPrincipal,
    resource: &str,
    action: &str,
    statement: &PolicyStatement,
) -> PolicyEvalResult {
    // TODO: implement support for wildcards and other matching patterns
    let principal_matches = statement.principal.iter().any(|p| p == principal);
    let action_matches = statement.action.iter().any(|a| a == action);
    let resource_matches = statement.resource.iter().any(|r| r == resource);

    if principal_matches && action_matches && resource_matches {
        match statement.effect.as_str() {
            "Allow" => PolicyEvalResult::Allowed,
            "Deny" => PolicyEvalResult::Denied,
            _ => PolicyEvalResult::NotApplicable,
        }
    } else {
        PolicyEvalResult::NotApplicable
    }
}

#[cfg(test)]
mod tests {
    use crate::auth::{Policy, PolicyEvalResult, PolicyPrincipal, evaluate_policy};

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

        let result = evaluate_policy(
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

        let result = evaluate_policy(
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

        let result = evaluate_policy(
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

        let result = evaluate_policy(
            &user_principal(),
            "arn:aws:sqs:us-east-1:123456789012:orders",
            "sqs:ReceiveMessage",
            &policy,
        );

        assert_eq!(result, PolicyEvalResult::NotApplicable);
    }
}
