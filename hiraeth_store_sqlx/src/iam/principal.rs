use async_trait::async_trait;
use hiraeth_store::{
    StoreError,
    iam::{Principal, PrincipalStore},
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

#[async_trait]
impl PrincipalStore for SqlitePrincipalStore {
    async fn get_principal(&self, principal_id: i64) -> Result<Option<Principal>, StoreError> {
        sqlx::query_as!(
            Principal,
            "SELECT id, account_id, kind, name, created_at FROM iam_principals WHERE id = ?",
            principal_id
        )
        .fetch_optional(&self.pool)
        .await
        .map_err(|err| StoreError::StorageFailure(err.to_string()))
    }

    async fn list_principals(&self) -> Result<Vec<Principal>, StoreError> {
        sqlx::query_as!(
            Principal,
            r#"
            SELECT id as "id!: i64", account_id, kind, name, created_at
            FROM iam_principals
            ORDER BY account_id ASC, kind ASC, name ASC
            "#
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|err| StoreError::StorageFailure(err.to_string()))
    }

    async fn create_principal(&self, principal: Principal) -> Result<(), StoreError> {
        sqlx::query!(
            "INSERT INTO iam_principals (account_id, kind, name) VALUES (?, ?, ?)",
            principal.account_id,
            principal.kind,
            principal.name,
        )
        .execute(&self.pool)
        .await
        .map_err(|err| StoreError::StorageFailure(err.to_string()))?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use tempfile::TempDir;

    use super::SqlitePrincipalStore;
    use crate::{get_store_pool, run_migrations};
    use hiraeth_store::iam::PrincipalStore;

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
    async fn run_migrations_adds_unique_constraint_for_logical_principal_identity() {
        let (_temp_dir, store) = test_store().await;

        let duplicate_insert = sqlx::query!(
            "INSERT INTO iam_principals (account_id, kind, name) VALUES (?, ?, ?)",
            "000000000000",
            "user",
            "test"
        )
        .execute(&store.pool)
        .await;

        assert!(duplicate_insert.is_err());
    }

    #[tokio::test]
    async fn list_principals_returns_seeded_and_inserted_principals() {
        let (_temp_dir, store) = test_store().await;

        sqlx::query!(
            "INSERT INTO iam_principals (account_id, kind, name) VALUES (?, ?, ?)",
            "000000000000",
            "role",
            "integration-runner"
        )
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
        }));
    }
}
