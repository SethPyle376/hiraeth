use async_trait::async_trait;

mod access_key_store;
mod principal;
mod principal_inline_policy_store;

pub use access_key_store::SqliteAccessKeyStore;
pub use principal::SqlitePrincipalStore;

use hiraeth_store::iam::{
    AccessKey, AccessKeyStore, NewPrincipal, Principal, PrincipalInlinePolicy,
    PrincipalInlinePolicyStore, PrincipalStore,
};

use crate::iam::principal_inline_policy_store::SqlitePrincipalInlinePolicyStore;

#[derive(Clone)]
pub struct SqliteIamStore {
    pub access_key_store: SqliteAccessKeyStore,
    pub principal_store: SqlitePrincipalStore,
    pub principal_inline_policy_store: SqlitePrincipalInlinePolicyStore,
    _pool: sqlx::SqlitePool,
}

impl SqliteIamStore {
    pub fn new(pool: &sqlx::SqlitePool) -> Self {
        Self {
            access_key_store: SqliteAccessKeyStore::new(pool),
            principal_store: SqlitePrincipalStore::new(pool),
            principal_inline_policy_store: SqlitePrincipalInlinePolicyStore::new(pool),
            _pool: pool.clone(),
        }
    }
}

#[async_trait]
impl AccessKeyStore for SqliteIamStore {
    async fn get_secret_key(
        &self,
        access_key: &str,
    ) -> Result<Option<AccessKey>, hiraeth_store::StoreError> {
        self.access_key_store.get_secret_key(access_key).await
    }

    async fn list_access_keys_for_principal(
        &self,
        principal_id: i64,
    ) -> Result<Vec<AccessKey>, hiraeth_store::StoreError> {
        self.access_key_store
            .list_access_keys_for_principal(principal_id)
            .await
    }

    async fn insert_secret_key(
        &self,
        access_key: &str,
        secret_key: &str,
        principal_id: i64,
    ) -> Result<AccessKey, hiraeth_store::StoreError> {
        self.access_key_store
            .insert_secret_key(access_key, secret_key, principal_id)
            .await
    }
}

#[async_trait]
impl PrincipalStore for SqliteIamStore {
    async fn get_principal(
        &self,
        principal_id: i64,
    ) -> Result<Option<Principal>, hiraeth_store::StoreError> {
        self.principal_store.get_principal(principal_id).await
    }

    async fn get_principal_by_identity(
        &self,
        account_id: &str,
        kind: &str,
        name: &str,
    ) -> Result<Option<Principal>, hiraeth_store::StoreError> {
        self.principal_store
            .get_principal_by_identity(account_id, kind, name)
            .await
    }

    async fn list_principals(&self) -> Result<Vec<Principal>, hiraeth_store::StoreError> {
        self.principal_store.list_principals().await
    }

    async fn create_principal(
        &self,
        principal: NewPrincipal,
    ) -> Result<Principal, hiraeth_store::StoreError> {
        self.principal_store.create_principal(principal).await
    }
}

#[async_trait]
impl PrincipalInlinePolicyStore for SqliteIamStore {
    async fn get_inline_policies_for_principal(
        &self,
        principal_id: i64,
    ) -> Result<Vec<PrincipalInlinePolicy>, hiraeth_store::StoreError> {
        self.principal_inline_policy_store
            .get_inline_policies_for_principal(principal_id)
            .await
    }
}

#[cfg(test)]
mod tests {
    use tempfile::TempDir;

    use crate::{get_store_pool, run_migrations};

    async fn test_pool() -> (TempDir, sqlx::SqlitePool) {
        let temp_dir = TempDir::new().expect("temp dir should be created");
        let db_url = format!(
            "sqlite://{}",
            temp_dir.path().join("store-test.sqlite").display()
        );
        let pool = get_store_pool(&db_url)
            .await
            .expect("pool should connect to temp sqlite db");
        run_migrations(&pool)
            .await
            .expect("migrations should run for temp sqlite db");

        (temp_dir, pool)
    }

    #[tokio::test]
    async fn run_migrations_creates_inline_policy_table_and_index() {
        let (_temp_dir, pool) = test_pool().await;

        let table_name = sqlx::query_scalar::<_, String>(
            "SELECT name FROM sqlite_master WHERE type = 'table' AND name = 'iam_principal_inline_policies'",
        )
        .fetch_one(&pool)
        .await
        .expect("iam_principal_inline_policies table should exist after migrations");

        let index_name = sqlx::query_scalar::<_, String>(
            "SELECT name FROM sqlite_master WHERE type = 'index' AND name = 'iam_principal_inline_policies_principal_id_idx'",
        )
        .fetch_one(&pool)
        .await
        .expect("iam_principal_inline_policies_principal_id_idx should exist after migrations");

        assert_eq!(table_name, "iam_principal_inline_policies");
        assert_eq!(index_name, "iam_principal_inline_policies_principal_id_idx");
    }

    #[tokio::test]
    async fn run_migrations_seeds_default_user_inline_policy() {
        let (_temp_dir, pool) = test_pool().await;

        let policy_name = sqlx::query_scalar::<_, String>(
            "SELECT policy_name
             FROM iam_principal_inline_policies
             WHERE principal_id = (
                 SELECT id FROM iam_principals
                 WHERE account_id = ? AND kind = ? AND name = ?
             )",
        )
        .bind("000000000000")
        .bind("user")
        .bind("test")
        .fetch_one(&pool)
        .await
        .expect("default inline policy should be seeded");

        let policy_document = sqlx::query_scalar::<_, String>(
            "SELECT policy_document
             FROM iam_principal_inline_policies
             WHERE principal_id = (
                 SELECT id FROM iam_principals
                 WHERE account_id = ? AND kind = ? AND name = ?
             )",
        )
        .bind("000000000000")
        .bind("user")
        .bind("test")
        .fetch_one(&pool)
        .await
        .expect("default inline policy document should be seeded");

        assert_eq!(policy_name, "default-account-admin");
        assert_eq!(
            policy_document,
            r#"{"Version":"2012-10-17","Statement":[{"Effect":"Allow","Action":"*","Resource":"arn:aws:*:*:000000000000:*"}]}"#
        );
    }
}
