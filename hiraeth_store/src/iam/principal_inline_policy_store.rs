use std::sync::RwLock;

use async_trait::async_trait;

use crate::StoreError;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrincipalInlinePolicy {
    pub id: i64,
    pub principal_id: i64,
    pub policy_name: String,
    pub policy_document: String,
    pub created_at: chrono::NaiveDateTime,
    pub updated_at: chrono::NaiveDateTime,
}

#[async_trait]
pub trait PrincipalInlinePolicyStore {
    async fn get_inline_policies_for_principal(
        &self,
        principal_id: i64,
    ) -> Result<Vec<PrincipalInlinePolicy>, StoreError>;
    async fn put_inline_policy(
        &self,
        principal_id: i64,
        policy_name: &str,
        policy_document: &str,
    ) -> Result<PrincipalInlinePolicy, StoreError>;
    async fn delete_inline_policy(
        &self,
        principal_id: i64,
        policy_name: &str,
    ) -> Result<(), StoreError>;
}

pub struct InMemoryPrincipalInlinePolicyStore {
    policies: RwLock<Vec<PrincipalInlinePolicy>>,
}

impl InMemoryPrincipalInlinePolicyStore {
    pub fn new(policies: impl IntoIterator<Item = PrincipalInlinePolicy>) -> Self {
        Self {
            policies: RwLock::new(policies.into_iter().collect()),
        }
    }
}

#[async_trait]
impl PrincipalInlinePolicyStore for InMemoryPrincipalInlinePolicyStore {
    async fn get_inline_policies_for_principal(
        &self,
        principal_id: i64,
    ) -> Result<Vec<PrincipalInlinePolicy>, StoreError> {
        Ok(self
            .policies
            .read()
            .expect("in-memory principal inline policy store read lock should not be poisoned")
            .iter()
            .filter(|p| p.principal_id == principal_id)
            .cloned()
            .collect())
    }

    async fn put_inline_policy(
        &self,
        principal_id: i64,
        policy_name: &str,
        policy_document: &str,
    ) -> Result<PrincipalInlinePolicy, StoreError> {
        let now = chrono::Utc::now().naive_utc();
        let mut policies = self
            .policies
            .write()
            .expect("in-memory principal inline policy store write lock should not be poisoned");

        if let Some(policy) = policies
            .iter_mut()
            .find(|policy| policy.principal_id == principal_id && policy.policy_name == policy_name)
        {
            policy.policy_document = policy_document.to_string();
            policy.updated_at = now;
            return Ok(policy.clone());
        }

        let next_id = policies.iter().map(|policy| policy.id).max().unwrap_or(0) + 1;
        let policy = PrincipalInlinePolicy {
            id: next_id,
            principal_id,
            policy_name: policy_name.to_string(),
            policy_document: policy_document.to_string(),
            created_at: now,
            updated_at: now,
        };
        policies.push(policy.clone());
        Ok(policy)
    }

    async fn delete_inline_policy(
        &self,
        principal_id: i64,
        policy_name: &str,
    ) -> Result<(), StoreError> {
        let mut policies = self
            .policies
            .write()
            .expect("in-memory principal inline policy store write lock should not be poisoned");

        let Some(index) = policies.iter().position(|policy| {
            policy.principal_id == principal_id && policy.policy_name == policy_name
        }) else {
            return Err(StoreError::NotFound(format!(
                "Inline policy {policy_name} not found for principal {principal_id}"
            )));
        };

        policies.remove(index);
        Ok(())
    }
}
