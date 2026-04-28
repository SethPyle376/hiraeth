use std::{collections::HashMap, sync::RwLock};

use crate::StoreError;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ManagedPolicy {
    pub id: i64,
    pub policy_id: String,
    pub account_id: String,
    pub policy_name: String,
    pub policy_path: Option<String>,
    pub policy_document: String,
    pub created_at: chrono::NaiveDateTime,
    pub updated_at: chrono::NaiveDateTime,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewManagedPolicy {
    pub policy_id: String,
    pub account_id: String,
    pub policy_name: String,
    pub policy_path: Option<String>,
    pub policy_document: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ManagedPolicyPrincipalAttachment {
    pub id: i64,
    pub policy_id: i64,
    pub principal_id: i64,
    pub created_at: chrono::NaiveDateTime,
}

#[async_trait::async_trait]
pub trait ManagedPolicyStore {
    async fn insert_managed_policy(
        &self,
        policy: NewManagedPolicy,
    ) -> Result<ManagedPolicy, StoreError>;

    async fn get_managed_policy(
        &self,
        account_id: &str,
        policy_name: &str,
        policy_path: &str,
    ) -> Result<Option<ManagedPolicy>, StoreError>;

    async fn list_managed_policies(&self) -> Result<Vec<ManagedPolicy>, StoreError>;

    async fn update_managed_policy_document(
        &self,
        policy_id: i64,
        policy_document: &str,
    ) -> Result<ManagedPolicy, StoreError>;

    async fn attach_policy_to_principal(
        &self,
        policy_id: i64,
        principal_id: i64,
    ) -> Result<(), StoreError>;

    async fn detach_policy_from_principal(
        &self,
        policy_id: i64,
        principal_id: i64,
    ) -> Result<(), StoreError>;

    async fn delete_managed_policy(
        &self,
        account_id: &str,
        policy_name: &str,
        policy_path: &str,
    ) -> Result<(), StoreError>;

    async fn get_managed_policies_attached_to_principal(
        &self,
        principal_id: i64,
    ) -> Result<Vec<ManagedPolicy>, StoreError>;
}

pub struct InMemoryManagedPolicyStore {
    policies: RwLock<HashMap<i64, ManagedPolicy>>,
    attachments: RwLock<Vec<ManagedPolicyPrincipalAttachment>>,
}

impl InMemoryManagedPolicyStore {
    pub fn new(
        policies: impl IntoIterator<Item = ManagedPolicy>,
        attachments: impl IntoIterator<Item = ManagedPolicyPrincipalAttachment>,
    ) -> Self {
        Self {
            policies: RwLock::new(policies.into_iter().map(|p| (p.id, p)).collect()),
            attachments: RwLock::new(attachments.into_iter().collect()),
        }
    }
}

#[async_trait::async_trait]
impl ManagedPolicyStore for InMemoryManagedPolicyStore {
    async fn insert_managed_policy(
        &self,
        policy: NewManagedPolicy,
    ) -> Result<ManagedPolicy, StoreError> {
        let mut policies = self
            .policies
            .write()
            .expect("in-memory managed policy store write lock should not be poisoned");
        let new_id = policies.keys().max().map_or(1, |max_id| max_id + 1);
        let now = chrono::Utc::now().naive_utc();
        let new_policy = ManagedPolicy {
            id: new_id,
            policy_id: policy.policy_id,
            account_id: policy.account_id,
            policy_name: policy.policy_name,
            policy_path: policy.policy_path,
            policy_document: policy.policy_document,
            created_at: now,
            updated_at: now,
        };
        policies.insert(new_id, new_policy.clone());
        Ok(new_policy)
    }

    async fn get_managed_policy(
        &self,
        account_id: &str,
        policy_name: &str,
        policy_path: &str,
    ) -> Result<Option<ManagedPolicy>, StoreError> {
        let policies = self
            .policies
            .read()
            .expect("in-memory managed policy store read lock should not be poisoned");
        Ok(policies
            .values()
            .find(|p| {
                p.account_id == account_id
                    && p.policy_name == policy_name
                    && p.policy_path.as_deref().unwrap_or("/") == policy_path
            })
            .cloned())
    }

    async fn list_managed_policies(&self) -> Result<Vec<ManagedPolicy>, StoreError> {
        let policies = self
            .policies
            .read()
            .expect("in-memory managed policy store read lock should not be poisoned");
        let mut policies = policies.values().cloned().collect::<Vec<_>>();
        policies.sort_by(|left, right| {
            left.account_id
                .cmp(&right.account_id)
                .then_with(|| {
                    left.policy_path
                        .as_deref()
                        .unwrap_or("/")
                        .cmp(right.policy_path.as_deref().unwrap_or("/"))
                })
                .then_with(|| left.policy_name.cmp(&right.policy_name))
        });
        Ok(policies)
    }

    async fn update_managed_policy_document(
        &self,
        policy_id: i64,
        policy_document: &str,
    ) -> Result<ManagedPolicy, StoreError> {
        let mut policies = self
            .policies
            .write()
            .expect("in-memory managed policy store write lock should not be poisoned");
        let policy = policies.get_mut(&policy_id).ok_or_else(|| {
            StoreError::NotFound(format!("Managed policy not found: {policy_id}"))
        })?;
        policy.policy_document = policy_document.to_string();
        policy.updated_at = chrono::Utc::now().naive_utc();
        Ok(policy.clone())
    }

    async fn attach_policy_to_principal(
        &self,
        policy_id: i64,
        principal_id: i64,
    ) -> Result<(), StoreError> {
        let mut attachments = self
            .attachments
            .write()
            .expect("in-memory managed policy store write lock should not be poisoned");
        let new_id = attachments
            .iter()
            .map(|a| a.id)
            .max()
            .map_or(1, |max_id| max_id + 1);
        let now = chrono::Utc::now().naive_utc();
        attachments.push(ManagedPolicyPrincipalAttachment {
            id: new_id,
            policy_id,
            principal_id,
            created_at: now,
        });
        Ok(())
    }

    async fn detach_policy_from_principal(
        &self,
        policy_id: i64,
        principal_id: i64,
    ) -> Result<(), StoreError> {
        let mut attachments = self
            .attachments
            .write()
            .expect("in-memory managed policy store write lock should not be poisoned");
        attachments.retain(|a| !(a.policy_id == policy_id && a.principal_id == principal_id));
        Ok(())
    }

    async fn delete_managed_policy(
        &self,
        account_id: &str,
        policy_name: &str,
        policy_path: &str,
    ) -> Result<(), StoreError> {
        let mut policies = self
            .policies
            .write()
            .expect("in-memory managed policy store write lock should not be poisoned");
        if let Some(policy) = policies
            .values()
            .find(|p| {
                p.account_id == account_id
                    && p.policy_name == policy_name
                    && p.policy_path.as_deref().unwrap_or("/") == policy_path
            })
            .cloned()
        {
            policies.remove(&policy.id);
            let mut attachments = self
                .attachments
                .write()
                .expect("in-memory managed policy store write lock should not be poisoned");
            attachments.retain(|a| a.policy_id != policy.id);
        }
        Ok(())
    }

    async fn get_managed_policies_attached_to_principal(
        &self,
        principal_id: i64,
    ) -> Result<Vec<ManagedPolicy>, StoreError> {
        let attachments = self
            .attachments
            .read()
            .expect("in-memory managed policy store read lock should not be poisoned");
        let policies = self
            .policies
            .read()
            .expect("in-memory managed policy store read lock should not be poisoned");
        let attached_policy_ids: Vec<i64> = attachments
            .iter()
            .filter(|a| a.principal_id == principal_id)
            .map(|a| a.policy_id)
            .collect();
        Ok(policies
            .values()
            .filter(|p| attached_policy_ids.contains(&p.id))
            .cloned()
            .collect())
    }
}
