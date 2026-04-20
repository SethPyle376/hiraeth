use serde::Deserialize;

use crate::auth::principal::{PolicyPrincipal, deserialize_principals};
use crate::auth::util::deserialize_one_or_many;

#[derive(Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "PascalCase")]
pub struct Policy {
    version: String,
    statement: Vec<PolicyStatement>,
}

impl Default for Policy {
    fn default() -> Self {
        Self {
            version: "2012-10-17".to_string(),
            statement: Vec::new(),
        }
    }
}

#[derive(Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "PascalCase")]
struct PolicyStatement {
    effect: String,
    #[serde(deserialize_with = "deserialize_one_or_many")]
    action: Vec<String>,
    #[serde(deserialize_with = "deserialize_one_or_many")]
    resource: Vec<String>,
    #[serde(deserialize_with = "deserialize_principals")]
    principal: Vec<PolicyPrincipal>,
}

#[cfg(test)]
mod tests {
    use super::{Policy, PolicyPrincipal, PolicyStatement};

    #[test]
    fn deserializes_policy_with_single_statement_and_scalar_fields() {
        let policy = serde_json::from_str::<Policy>(
            r#"{
                "Version": "2012-10-17",
                "Statement": [
                    {
                        "Effect": "Allow",
                        "Principal": "*",
                        "Action": "sqs:SendMessage",
                        "Resource": "arn:aws:sqs:us-east-1:000000000000:orders"
                    }
                ]
            }"#,
        )
        .expect("policy should deserialize");

        assert_eq!(policy.version, "2012-10-17");
        assert_eq!(
            policy.statement,
            vec![PolicyStatement {
                effect: "Allow".to_string(),
                principal: vec![PolicyPrincipal::Any],
                action: vec!["sqs:SendMessage".to_string()],
                resource: vec!["arn:aws:sqs:us-east-1:000000000000:orders".to_string()],
            }]
        );
    }

    #[test]
    fn deserializes_policy_with_multiple_statements_and_array_fields() {
        let policy = serde_json::from_str::<Policy>(
            r#"{
                "Version": "2012-10-17",
                "Statement": [
                    {
                        "Effect": "Allow",
                        "Principal": {
                            "AWS": [
                                "123456789012",
                                "arn:aws:iam::210987654321:role/app-worker"
                            ],
                            "Service": "sns.amazonaws.com"
                        },
                        "Action": [
                            "sqs:SendMessage",
                            "sqs:ReceiveMessage"
                        ],
                        "Resource": [
                            "arn:aws:sqs:us-east-1:000000000000:orders",
                            "arn:aws:sqs:us-east-1:000000000000:payments"
                        ]
                    },
                    {
                        "Effect": "Deny",
                        "Principal": {
                            "AWS": "arn:aws:sts::123456789012:assumed-role/app/session"
                        },
                        "Action": "sqs:DeleteQueue",
                        "Resource": "*"
                    }
                ]
            }"#,
        )
        .expect("policy should deserialize");

        assert_eq!(policy.statement.len(), 2);
        assert_eq!(
            policy.statement[0],
            PolicyStatement {
                effect: "Allow".to_string(),
                principal: vec![
                    PolicyPrincipal::Account("123456789012".to_string()),
                    PolicyPrincipal::Role {
                        account_id: "210987654321".to_string(),
                        role_name: "app-worker".to_string(),
                    },
                    PolicyPrincipal::Service("sns.amazonaws.com".to_string()),
                ],
                action: vec![
                    "sqs:SendMessage".to_string(),
                    "sqs:ReceiveMessage".to_string()
                ],
                resource: vec![
                    "arn:aws:sqs:us-east-1:000000000000:orders".to_string(),
                    "arn:aws:sqs:us-east-1:000000000000:payments".to_string(),
                ],
            }
        );
        assert_eq!(
            policy.statement[1],
            PolicyStatement {
                effect: "Deny".to_string(),
                principal: vec![PolicyPrincipal::AssumedRole {
                    account_id: "123456789012".to_string(),
                    role_name: "app".to_string(),
                    session_name: "session".to_string(),
                }],
                action: vec!["sqs:DeleteQueue".to_string()],
                resource: vec!["*".to_string()],
            }
        );
    }

    #[test]
    fn rejects_scalar_statement_shape_for_now() {
        let result = serde_json::from_str::<Policy>(
            r#"{
                "Version": "2012-10-17",
                "Statement": {
                    "Effect": "Allow",
                    "Principal": "*",
                    "Action": "sqs:SendMessage",
                    "Resource": "*"
                }
            }"#,
        );

        assert!(result.is_err());
    }

    #[test]
    fn rejects_statement_with_invalid_principal() {
        let result = serde_json::from_str::<Policy>(
            r#"{
                "Version": "2012-10-17",
                "Statement": [
                    {
                        "Effect": "Allow",
                        "Principal": "not-a-wildcard",
                        "Action": "sqs:SendMessage",
                        "Resource": "*"
                    }
                ]
            }"#,
        );

        assert!(result.is_err());
    }

    #[test]
    fn rejects_scalar_action_with_non_string_value() {
        let result = serde_json::from_str::<Policy>(
            r#"{
                "Version": "2012-10-17",
                "Statement": [
                    {
                        "Effect": "Allow",
                        "Principal": "*",
                        "Action": 123,
                        "Resource": "*"
                    }
                ]
            }"#,
        );

        assert!(result.is_err());
    }

    #[test]
    fn rejects_resource_with_mixed_array_types() {
        let result = serde_json::from_str::<Policy>(
            r#"{
                "Version": "2012-10-17",
                "Statement": [
                    {
                        "Effect": "Allow",
                        "Principal": "*",
                        "Action": "sqs:SendMessage",
                        "Resource": ["*", 123]
                    }
                ]
            }"#,
        );

        assert!(result.is_err());
    }
}
