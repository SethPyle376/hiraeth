use async_trait::async_trait;
use hiraeth_store::{
    StoreError,
    iam::{NewPrincipal, Principal, PrincipalStore},
};

#[derive(Clone)]
pub struct SqlitePrincipalStore {
    pool: sqlx::SqlitePool,
}

impl SqlitePrincipalStore {
    pub fn new(pool: &sqlx::SqlitePool) -> Self {
        Self { pool: pool.clone() }
    }
}

fn map_sqlx_error(err: sqlx::Error) -> StoreError {
    if let sqlx::Error::Database(database_error) = &err
        && database_error.is_unique_violation()
    {
        return StoreError::Conflict(database_error.message().to_string());
    }

    StoreError::StorageFailure(err.to_string())
}

#[async_trait]
impl PrincipalStore for SqlitePrincipalStore {
    async fn get_principal(&self, principal_id: i64) -> Result<Option<Principal>, StoreError> {
        sqlx::query_as!(
            Principal,
            "SELECT id, account_id, kind, name, path, user_id, created_at FROM iam_principals WHERE id = ?",
            principal_id
        )
        .fetch_optional(&self.pool)
        .await
        .map_err(map_sqlx_error)
    }

    async fn get_principal_by_identity(
        &self,
        account_id: &str,
        kind: &str,
        name: &str,
    ) -> Result<Option<Principal>, StoreError> {
        sqlx::query_as!(
            Principal,
            r#"
            SELECT id as "id!: i64", account_id, kind, name, path, user_id, created_at
            FROM iam_principals
            WHERE account_id = ? AND kind = ? AND name = ?
            "#,
            account_id,
            kind,
            name
        )
        .fetch_optional(&self.pool)
        .await
        .map_err(map_sqlx_error)
    }

    async fn list_principals(&self) -> Result<Vec<Principal>, StoreError> {
        sqlx::query_as!(
            Principal,
            r#"
            SELECT id as "id!: i64", account_id, kind, name, path, user_id, created_at
            FROM iam_principals
            ORDER BY account_id ASC, kind ASC, name ASC
            "#
        )
        .fetch_all(&self.pool)
        .await
        .map_err(map_sqlx_error)
    }

    async fn create_principal(&self, principal: NewPrincipal) -> Result<Principal, StoreError> {
        sqlx::query_as!(
            Principal,
            r#"
            INSERT INTO iam_principals (account_id, kind, name, path, user_id)
            VALUES (?, ?, ?, ?, ?)
            RETURNING id as "id!: i64", account_id, kind, name, path, user_id, created_at
            "#,
            principal.account_id,
            principal.kind,
            principal.name,
            principal.path,
            principal.user_id,
        )
        .fetch_one(&self.pool)
        .await
        .map_err(map_sqlx_error)
    }

    async fn delete_principal(&self, principal_id: i64) -> Result<(), StoreError> {
        let result = sqlx::query("DELETE FROM iam_principals WHERE id = ?")
            .bind(principal_id)
            .execute(&self.pool)
            .await
            .map_err(map_sqlx_error)?;

        if result.rows_affected() == 0 {
            return Err(StoreError::NotFound(format!(
                "Principal {principal_id} not found"
            )));
        }

        Ok(())
    }

    async fn delete_user(&self, account_id: &str, name: &str) -> Result<(), StoreError> {
        let result = sqlx::query!(
            "DELETE FROM iam_principals WHERE kind = 'user' AND account_id = ? AND name = ?",
            account_id,
            name
        )
        .execute(&self.pool)
        .await
        .map_err(map_sqlx_error)?;

        if result.rows_affected() == 0 {
            return Err(StoreError::NotFound(format!(
                "User with account_id {} and name {} not found",
                account_id, name
            )));
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use tempfile::TempDir;

    use super::SqlitePrincipalStore;
    use crate::{get_store_pool, run_migrations};
    use hiraeth_store::{StoreError, iam::PrincipalStore};

    async fn test_store() -> (TempDir, SqlitePrincipalStore) {
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

        (temp_dir, SqlitePrincipalStore::new(&pool))
    }

    #[tokio::test]
    async fn run_migrations_creates_iam_principals_table() {
        let (_temp_dir, store) = test_store().await;

        let table_name = sqlx::query_scalar::<_, String>(
            "SELECT name FROM sqlite_master WHERE type = 'table' AND name = 'iam_principals'",
        )
        .fetch_one(&store.pool)
        .await
        .expect("iam_principals table should exist after migrations");

        assert_eq!(table_name, "iam_principals");
    }

    #[tokio::test]
    async fn get_principal_returns_seeded_principal() {
        let (_temp_dir, store) = test_store().await;

        let principal_id = sqlx::query_scalar::<_, i64>(
            "SELECT id FROM iam_principals WHERE account_id = ? AND kind = ? AND name = ?",
        )
        .bind("000000000000")
        .bind("user")
        .bind("test")
        .fetch_one(&store.pool)
        .await
        .expect("seeded principal id should exist");

        let principal = store
            .get_principal(principal_id)
            .await
            .expect("principal lookup should succeed")
            .expect("principal should exist");

        assert_eq!(principal.id, principal_id);
        assert_eq!(principal.account_id, "000000000000");
        assert_eq!(principal.kind, "user");
        assert_eq!(principal.name, "test");
        assert_eq!(principal.path, "/");
        assert!(principal.user_id.starts_with("AIDA"));
    }

    #[tokio::test]
    async fn get_principal_returns_none_for_missing_principal() {
        let (_temp_dir, store) = test_store().await;

        let principal = store
            .get_principal(999)
            .await
            .expect("principal lookup should succeed");

        assert!(principal.is_none());
    }

    #[tokio::test]
    async fn get_principal_by_identity_returns_matching_principal() {
        let (_temp_dir, store) = test_store().await;

        let principal = store
            .get_principal_by_identity("000000000000", "user", "test")
            .await
            .expect("principal lookup should succeed")
            .expect("principal should exist");

        assert_eq!(principal.account_id, "000000000000");
        assert_eq!(principal.kind, "user");
        assert_eq!(principal.name, "test");
        assert_eq!(principal.path, "/");
    }

    #[tokio::test]
    async fn get_principal_by_identity_returns_none_for_missing_principal() {
        let (_temp_dir, store) = test_store().await;

        let principal = store
            .get_principal_by_identity("000000000000", "user", "missing")
            .await
            .expect("principal lookup should succeed");

        assert!(principal.is_none());
    }

    #[tokio::test]
    async fn run_migrations_adds_unique_constraint_for_logical_principal_identity() {
        let (_temp_dir, store) = test_store().await;

        let duplicate_insert = sqlx::query(
            "INSERT INTO iam_principals (account_id, kind, name, path, user_id) VALUES (?, ?, ?, ?, ?)",
        )
        .bind("000000000000")
        .bind("user")
        .bind("test")
        .bind("/")
        .bind("AIDADUPLICATETEST01")
        .execute(&store.pool)
        .await;

        assert!(duplicate_insert.is_err());
    }

    #[tokio::test]
    async fn list_principals_returns_seeded_and_inserted_principals() {
        let (_temp_dir, store) = test_store().await;

        sqlx::query(
            "INSERT INTO iam_principals (account_id, kind, name, path, user_id) VALUES (?, ?, ?, ?, ?)",
        )
        .bind("000000000000")
        .bind("role")
        .bind("integration-runner")
        .bind("/service-role/")
        .bind("AIDAINTEGRATIONRUN01")
        .execute(&store.pool)
        .await
        .expect("inserted principal should succeed");

        let principals = store
            .list_principals()
            .await
            .expect("listing principals should succeed");

        assert!(principals.iter().any(|principal| {
            principal.account_id == "000000000000"
                && principal.kind == "user"
                && principal.name == "test"
        }));
        assert!(principals.iter().any(|principal| {
            principal.account_id == "000000000000"
                && principal.kind == "role"
                && principal.name == "integration-runner"
                && principal.path == "/service-role/"
        }));
    }

    #[tokio::test]
    async fn delete_user_deletes_matching_user() {
        let (_temp_dir, store) = test_store().await;

        store
            .delete_user("000000000000", "test")
            .await
            .expect("delete user should succeed");

        let deleted = store
            .get_principal_by_identity("000000000000", "user", "test")
            .await
            .expect("principal lookup should succeed");

        assert!(deleted.is_none());
    }

    #[tokio::test]
    async fn delete_user_does_not_delete_other_accounts_or_kinds() {
        let (_temp_dir, store) = test_store().await;

        sqlx::query(
            "INSERT INTO iam_principals (account_id, kind, name, path, user_id) VALUES (?, ?, ?, ?, ?)",
        )
        .bind("111111111111")
        .bind("user")
        .bind("test")
        .bind("/")
        .bind("AIDAOTHERACCOUNT001")
        .execute(&store.pool)
        .await
        .expect("other account user insert should succeed");
        sqlx::query(
            "INSERT INTO iam_principals (account_id, kind, name, path, user_id) VALUES (?, ?, ?, ?, ?)",
        )
        .bind("000000000000")
        .bind("role")
        .bind("test")
        .bind("/")
        .bind("AIDAROLEWITHSAMENAME")
        .execute(&store.pool)
        .await
        .expect("same-name role insert should succeed");

        store
            .delete_user("000000000000", "test")
            .await
            .expect("delete user should succeed");

        assert!(
            store
                .get_principal_by_identity("111111111111", "user", "test")
                .await
                .expect("principal lookup should succeed")
                .is_some()
        );
        assert!(
            store
                .get_principal_by_identity("000000000000", "role", "test")
                .await
                .expect("principal lookup should succeed")
                .is_some()
        );
    }

    #[tokio::test]
    async fn delete_user_returns_not_found_for_missing_user() {
        let (_temp_dir, store) = test_store().await;

        let error = store
            .delete_user("000000000000", "missing")
            .await
            .expect_err("missing user should return not found");

        assert_eq!(
            error,
            StoreError::NotFound(
                "User with account_id 000000000000 and name missing not found".to_string()
            )
        );
    }

    #[tokio::test]
    async fn delete_user_cascades_access_keys_and_inline_policies() {
        let (_temp_dir, store) = test_store().await;

        let principal_id = sqlx::query_scalar::<_, i64>(
            "SELECT id FROM iam_principals WHERE account_id = ? AND kind = ? AND name = ?",
        )
        .bind("000000000000")
        .bind("user")
        .bind("test")
        .fetch_one(&store.pool)
        .await
        .expect("seeded principal should exist");

        store
            .delete_user("000000000000", "test")
            .await
            .expect("delete user should succeed");

        let access_key_count = sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM iam_access_keys WHERE principal_id = ?",
        )
        .bind(principal_id)
        .fetch_one(&store.pool)
        .await
        .expect("access key count should succeed");
        let inline_policy_count = sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM iam_principal_inline_policies WHERE principal_id = ?",
        )
        .bind(principal_id)
        .fetch_one(&store.pool)
        .await
        .expect("inline policy count should succeed");

        assert_eq!(access_key_count, 0);
        assert_eq!(inline_policy_count, 0);
    }
}
