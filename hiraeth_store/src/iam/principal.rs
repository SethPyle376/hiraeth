use std::{collections::HashMap, sync::RwLock};

use async_trait::async_trait;

use crate::StoreError;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Principal {
    pub id: i64,
    pub account_id: String,
    pub kind: String,
    pub name: String,
    pub path: String,
    pub user_id: String,
    pub created_at: chrono::NaiveDateTime,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewPrincipal {
    pub account_id: String,
    pub kind: String,
    pub name: String,
    pub path: String,
    pub user_id: String,
}

#[allow(async_fn_in_trait)]
#[async_trait]
pub trait PrincipalStore {
    async fn get_principal(&self, principal_id: i64) -> Result<Option<Principal>, StoreError>;
    async fn get_principal_by_identity(
        &self,
        account_id: &str,
        kind: &str,
        name: &str,
    ) -> Result<Option<Principal>, StoreError>;
    async fn list_principals(&self) -> Result<Vec<Principal>, StoreError>;
    async fn create_principal(&self, principal: NewPrincipal) -> Result<Principal, StoreError>;
}

pub struct InMemoryPrincipalStore {
    principals: RwLock<HashMap<i64, Principal>>,
}

impl InMemoryPrincipalStore {
    pub fn new(principals: impl IntoIterator<Item = Principal>) -> Self {
        Self {
            principals: RwLock::new(principals.into_iter().map(|p| (p.id, p)).collect()),
        }
    }
}

#[async_trait]
impl PrincipalStore for InMemoryPrincipalStore {
    async fn get_principal(&self, principal_id: i64) -> Result<Option<Principal>, StoreError> {
        Ok(self
            .principals
            .read()
            .expect("in-memory principal store read lock should not be poisoned")
            .get(&principal_id)
            .cloned())
    }

    async fn get_principal_by_identity(
        &self,
        account_id: &str,
        kind: &str,
        name: &str,
    ) -> Result<Option<Principal>, StoreError> {
        Ok(self
            .principals
            .read()
            .expect("in-memory principal store read lock should not be poisoned")
            .values()
            .find(|principal| {
                principal.account_id == account_id
                    && principal.kind == kind
                    && principal.name == name
            })
            .cloned())
    }

    async fn list_principals(&self) -> Result<Vec<Principal>, StoreError> {
        let mut principals = self
            .principals
            .read()
            .expect("in-memory principal store read lock should not be poisoned")
            .values()
            .cloned()
            .collect::<Vec<_>>();
        principals.sort_by(|left, right| {
            left.account_id
                .cmp(&right.account_id)
                .then(left.kind.cmp(&right.kind))
                .then(left.name.cmp(&right.name))
        });
        Ok(principals)
    }

    async fn create_principal(&self, principal: NewPrincipal) -> Result<Principal, StoreError> {
        let mut principals = self
            .principals
            .write()
            .expect("in-memory principal store write lock should not be poisoned");

        if principals.values().any(|existing| {
            existing.account_id == principal.account_id
                && existing.kind == principal.kind
                && existing.name == principal.name
        }) {
            return Err(StoreError::Conflict(format!(
                "Principal {} {} {} already exists",
                principal.account_id, principal.kind, principal.name
            )));
        }
        let next_id = principals.keys().copied().max().unwrap_or(0) + 1;
        let created_principal = Principal {
            id: next_id,
            account_id: principal.account_id,
            kind: principal.kind,
            name: principal.name,
            path: principal.path,
            user_id: principal.user_id,
            created_at: chrono::Utc::now().naive_utc(),
        };
        principals.insert(next_id, created_principal.clone());
        Ok(created_principal)
    }
}
