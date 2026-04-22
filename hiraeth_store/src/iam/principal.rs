use std::collections::HashMap;

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
pub trait PrincipalStore {
    async fn get_principal(&self, principal_id: i64) -> Result<Option<Principal>, StoreError>;
}

pub struct InMemoryPrincipalStore {
    principals: HashMap<i64, Principal>,
}

impl InMemoryPrincipalStore {
    pub fn new(principals: impl IntoIterator<Item = Principal>) -> Self {
        Self {
            principals: principals.into_iter().map(|p| (p.id, p)).collect(),
        }
    }
}

impl PrincipalStore for InMemoryPrincipalStore {
    async fn get_principal(&self, principal_id: i64) -> Result<Option<Principal>, StoreError> {
        Ok(self.principals.get(&principal_id).cloned())
    }
}
