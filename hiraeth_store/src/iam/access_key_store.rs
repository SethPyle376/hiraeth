use std::collections::HashMap;

use crate::StoreError;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AccessKey {
    pub key_id: String,
    pub principal_id: i64,
    pub secret_key: String,
    pub created_at: chrono::NaiveDateTime,
}

#[allow(async_fn_in_trait)]
pub trait AccessKeyStore {
    async fn get_secret_key(&self, access_key: &str) -> Result<Option<AccessKey>, StoreError>;
    async fn list_access_keys_for_principal(
        &self,
        principal_id: i64,
    ) -> Result<Vec<AccessKey>, StoreError>;
    async fn insert_secret_key(
        &mut self,
        access_key: &str,
        secret_key: &str,
        principal_id: i64,
    ) -> Result<(), StoreError>;
}

pub struct InMemoryAccessKeyStore {
    keys: HashMap<String, AccessKey>,
}

impl InMemoryAccessKeyStore {
    pub fn new(keys: impl IntoIterator<Item = AccessKey>) -> Self {
        Self {
            keys: keys.into_iter().map(|k| (k.key_id.clone(), k)).collect(),
        }
    }
}

impl AccessKeyStore for InMemoryAccessKeyStore {
    async fn get_secret_key(&self, access_key: &str) -> Result<Option<AccessKey>, StoreError> {
        Ok(self.keys.get(access_key).cloned())
    }

    async fn list_access_keys_for_principal(
        &self,
        principal_id: i64,
    ) -> Result<Vec<AccessKey>, StoreError> {
        let mut keys = self
            .keys
            .values()
            .filter(|key| key.principal_id == principal_id)
            .cloned()
            .collect::<Vec<_>>();
        keys.sort_by(|left, right| left.key_id.cmp(&right.key_id));
        Ok(keys)
    }

    async fn insert_secret_key(
        &mut self,
        access_key: &str,
        secret_key: &str,
        principal_id: i64,
    ) -> Result<(), StoreError> {
        let new_key = AccessKey {
            key_id: access_key.to_string(),
            secret_key: secret_key.to_string(),
            principal_id,
            created_at: chrono::Utc::now().naive_utc(),
        };
        self.keys.insert(access_key.to_string(), new_key);
        Ok(())
    }
}
