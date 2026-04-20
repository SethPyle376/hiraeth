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
