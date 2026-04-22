mod access_key_store;
mod principal;

pub use access_key_store::SqliteAccessKeyStore;
pub use principal::SqlitePrincipalStore;

use hiraeth_store::iam::{AccessKey, AccessKeyStore, Principal, PrincipalStore};

#[derive(Clone)]
pub struct SqliteIamStore {
    pub access_key_store: SqliteAccessKeyStore,
    pub principal_store: SqlitePrincipalStore,
    _pool: sqlx::SqlitePool,
}

impl SqliteIamStore {
    pub fn new(pool: &sqlx::SqlitePool) -> Self {
        Self {
            access_key_store: SqliteAccessKeyStore::new(pool),
            principal_store: SqlitePrincipalStore::new(pool),
            _pool: pool.clone(),
        }
    }
}

impl AccessKeyStore for SqliteIamStore {
    async fn get_secret_key(
        &self,
        access_key: &str,
    ) -> Result<Option<AccessKey>, hiraeth_store::StoreError> {
        self.access_key_store.get_secret_key(access_key).await
    }

    async fn insert_secret_key(
        &mut self,
        access_key: &str,
        secret_key: &str,
        principal_id: i64,
    ) -> Result<(), hiraeth_store::StoreError> {
        self.access_key_store
            .insert_secret_key(access_key, secret_key, principal_id)
            .await
    }
}

impl PrincipalStore for SqliteIamStore {
    async fn get_principal(
        &self,
        principal_id: i64,
    ) -> Result<Option<Principal>, hiraeth_store::StoreError> {
        self.principal_store.get_principal(principal_id).await
    }
}
