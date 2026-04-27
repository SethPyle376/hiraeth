use std::{collections::HashMap, sync::RwLock};

use crate::StoreError;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ManagedPolicy {
    pub id: i64,
    pub account_id: String,
    pub policy_name: String,
    pub policy_path: Option<String>,
    pub policy_document: String,
    pub created_at: chrono::NaiveDateTime,
    pub updated_at: chrono::NaiveDateTime,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewManagedPolicy {
    pub account_id: String,
    pub policy_name: String,
    pub policy_path: Option<String>,
    pub policy_document: String,
}

#[async_trait::async_trait]
pub trait ManagedPolicyStore {
    async fn insert_managed_policy(
        &self,
        policy: NewManagedPolicy,
    ) -> Result<ManagedPolicy, StoreError>;
}

pub struct InMemoryManagedPolicyStore {
    policies: RwLock<HashMap<i64, ManagedPolicy>>,
}

impl InMemoryManagedPolicyStore {
    pub fn new(policies: impl IntoIterator<Item = ManagedPolicy>) -> Self {
        Self {
            policies: RwLock::new(policies.into_iter().map(|p| (p.id, p)).collect()),
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
}
