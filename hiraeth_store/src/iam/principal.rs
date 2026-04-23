use std::{collections::HashMap, sync::RwLock};

use async_trait::async_trait;

use crate::StoreError;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Principal {
    pub id: i64,
    pub account_id: String,
    pub kind: String,
    pub name: String,
    pub created_at: chrono::NaiveDateTime,
}

#[allow(async_fn_in_trait)]
#[async_trait]
pub trait PrincipalStore {
    async fn get_principal(&self, principal_id: i64) -> Result<Option<Principal>, StoreError>;
    async fn list_principals(&self) -> Result<Vec<Principal>, StoreError>;
    async fn create_principal(&self, principal: Principal) -> Result<(), StoreError>;
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

    async fn create_principal(&self, principal: Principal) -> Result<(), StoreError> {
        let mut principals = self
            .principals
            .write()
            .expect("in-memory principal store write lock should not be poisoned");

        if principals.contains_key(&principal.id) {
            return Err(StoreError::Conflict(format!(
                "Principal with id {} already exists",
                principal.id
            )));
        }
        principals.insert(principal.id, principal);
        Ok(())
    }
}
