use std::{collections::HashMap, sync::RwLock};

use async_trait::async_trait;

use crate::StoreError;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AccessKey {
    pub key_id: String,
    pub principal_id: i64,
    pub secret_key: String,
    pub created_at: chrono::NaiveDateTime,
}

#[async_trait]
pub trait AccessKeyStore {
    async fn get_secret_key(&self, access_key: &str) -> Result<Option<AccessKey>, StoreError>;
    async fn list_access_keys_for_principal(
        &self,
        principal_id: i64,
    ) -> Result<Vec<AccessKey>, StoreError>;
    async fn insert_secret_key(
        &self,
        access_key: &str,
        secret_key: &str,
        principal_id: i64,
    ) -> Result<AccessKey, StoreError>;
}

pub struct InMemoryAccessKeyStore {
    keys: RwLock<HashMap<String, AccessKey>>,
}

impl InMemoryAccessKeyStore {
    pub fn new(keys: impl IntoIterator<Item = AccessKey>) -> Self {
        Self {
            keys: RwLock::new(keys.into_iter().map(|k| (k.key_id.clone(), k)).collect()),
        }
    }
}

#[async_trait]
impl AccessKeyStore for InMemoryAccessKeyStore {
    async fn get_secret_key(&self, access_key: &str) -> Result<Option<AccessKey>, StoreError> {
        Ok(self
            .keys
            .read()
            .expect("in-memory access key store read lock should not be poisoned")
            .get(access_key)
            .cloned())
    }

    async fn list_access_keys_for_principal(
        &self,
        principal_id: i64,
    ) -> Result<Vec<AccessKey>, StoreError> {
        let mut keys = self
            .keys
            .read()
            .expect("in-memory access key store read lock should not be poisoned")
            .values()
            .filter(|key| key.principal_id == principal_id)
            .cloned()
            .collect::<Vec<_>>();
        keys.sort_by_key(|key| key.key_id.clone());
        Ok(keys)
    }

    async fn insert_secret_key(
        &self,
        access_key: &str,
        secret_key: &str,
        principal_id: i64,
    ) -> Result<AccessKey, StoreError> {
        let mut keys = self
            .keys
            .write()
            .expect("in-memory access key store write lock should not be poisoned");
        if keys.contains_key(access_key) {
            return Err(StoreError::Conflict(format!(
                "Access key {access_key} already exists"
            )));
        }

        let new_key = AccessKey {
            key_id: access_key.to_string(),
            secret_key: secret_key.to_string(),
            principal_id,
            created_at: chrono::Utc::now().naive_utc(),
        };
        keys.insert(access_key.to_string(), new_key.clone());
        Ok(new_key)
    }
}
