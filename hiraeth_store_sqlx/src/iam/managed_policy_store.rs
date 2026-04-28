use async_trait::async_trait;
use hiraeth_store::{
    StoreError,
    iam::{ManagedPolicy, ManagedPolicyStore, NewManagedPolicy},
};

use crate::iam::map_sqlx_error;

#[derive(Clone)]
pub struct SqliteManagedPolicyStore {
    pool: sqlx::SqlitePool,
}

impl SqliteManagedPolicyStore {
    pub fn new(pool: &sqlx::SqlitePool) -> Self {
        Self { pool: pool.clone() }
    }
}

#[async_trait]
impl ManagedPolicyStore for SqliteManagedPolicyStore {
    async fn insert_managed_policy(
        &self,
        policy: NewManagedPolicy,
    ) -> Result<ManagedPolicy, StoreError> {
        sqlx::query_as!(
                ManagedPolicy,
                r#"
                INSERT INTO iam_managed_policies (policy_id, account_id, policy_name, policy_path, policy_document, created_at, updated_at)
                VALUES (?, ?, ?, ?, ?, CURRENT_TIMESTAMP, CURRENT_TIMESTAMP)
                RETURNING id, policy_id, account_id, policy_name, policy_path, policy_document, created_at, updated_at
                "#,
                policy.policy_id,
                policy.account_id,
                policy.policy_name,
                policy.policy_path,
                policy.policy_document
            )
            .fetch_one(&self.pool)
            .await
            .map_err(map_sqlx_error)
    }

    async fn get_managed_policy(
        &self,
        account_id: &str,
        policy_name: &str,
        policy_path: &str,
    ) -> Result<Option<ManagedPolicy>, StoreError> {
        sqlx::query_as!(
                ManagedPolicy,
                r#"
                SELECT id, policy_id, account_id, policy_name, policy_path, policy_document, created_at, updated_at
                FROM iam_managed_policies
                WHERE account_id = ? AND policy_name = ? AND policy_path = ?
                "#,
                account_id,
                policy_name,
                policy_path
            )
            .fetch_optional(&self.pool)
            .await
            .map_err(map_sqlx_error)
    }

    async fn attach_policy_to_principal(
        &self,
        policy_id: i64,
        principal_id: i64,
    ) -> Result<(), StoreError> {
        sqlx::query!(
            r#"
            INSERT INTO iam_user_policy_attachments (policy_id, user_id, created_at)
            VALUES (?, ?, CURRENT_TIMESTAMP)
            "#,
            policy_id,
            principal_id
        )
        .execute(&self.pool)
        .await
        .map_err(map_sqlx_error)
        .map(|_| ())
    }

    async fn detach_policy_from_principal(
        &self,
        policy_id: i64,
        principal_id: i64,
    ) -> Result<(), StoreError> {
        let result = sqlx::query!(
            r#"
            DELETE FROM iam_user_policy_attachments
            WHERE policy_id = ? AND user_id = ?
            "#,
            policy_id,
            principal_id
        )
        .execute(&self.pool)
        .await
        .map_err(map_sqlx_error)?;

        if result.rows_affected() == 0 {
            Err(StoreError::NotFound(format!(
                "No attachment found for policy_id {} and principal_id {}",
                policy_id, principal_id
            )))
        } else {
            Ok(())
        }
    }

    async fn delete_managed_policy(
        &self,
        account_id: &str,
        policy_name: &str,
        policy_path: &str,
    ) -> Result<(), StoreError> {
        let result = sqlx::query!(
            r#"
            DELETE FROM iam_managed_policies
            WHERE account_id = ? AND policy_name = ? AND policy_path = ?
            "#,
            account_id,
            policy_name,
            policy_path
        )
        .execute(&self.pool)
        .await
        .map_err(map_sqlx_error)?;

        if result.rows_affected() == 0 {
            Err(StoreError::NotFound(format!(
                "Managed policy {} (path {}) not found for account {}",
                policy_name, policy_path, account_id
            )))
        } else {
            Ok(())
        }
    }

    async fn get_managed_policies_attached_to_principal(
        &self,
        principal_id: i64,
    ) -> Result<Vec<ManagedPolicy>, StoreError> {
        sqlx::query_as!(
                ManagedPolicy,
                r#"
                SELECT mp.id, mp.policy_id, mp.account_id, mp.policy_name, mp.policy_path, mp.policy_document, mp.created_at, mp.updated_at
                FROM iam_managed_policies mp
                JOIN iam_user_policy_attachments a ON a.policy_id = mp.id
                WHERE a.user_id = ?
                "#,
                principal_id
            )
            .fetch_all(&self.pool)
            .await
            .map_err(map_sqlx_error)
    }
}

#[cfg(test)]
mod tests {
    use tempfile::TempDir;

    use hiraeth_store::iam::{ManagedPolicyStore, NewManagedPolicy, NewPrincipal, PrincipalStore};

    use crate::{get_store_pool, run_migrations};

    use super::super::SqlitePrincipalStore;
    use super::SqliteManagedPolicyStore;

    async fn test_pool() -> (TempDir, sqlx::SqlitePool) {
        let temp_dir = TempDir::new().expect("temp dir should be created");
        let db_url = format!(
            "sqlite://{}",
            temp_dir.path().join("managed-policy.sqlite").display()
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
    async fn get_managed_policy_is_path_aware() {
        let (_temp_dir, pool) = test_pool().await;
        let store = SqliteManagedPolicyStore::new(&pool);

        store
            .insert_managed_policy(NewManagedPolicy {
                policy_id: "AIDAPOLICY00000001".to_string(),
                account_id: "123456789012".to_string(),
                policy_name: "orders-readonly".to_string(),
                policy_path: Some("/team-a/".to_string()),
                policy_document: r#"{"Version":"2012-10-17","Statement":[]}"#.to_string(),
            })
            .await
            .expect("insert should succeed");
        store
            .insert_managed_policy(NewManagedPolicy {
                policy_id: "AIDAPOLICY00000002".to_string(),
                account_id: "123456789012".to_string(),
                policy_name: "orders-readonly".to_string(),
                policy_path: Some("/team-b/".to_string()),
                policy_document: r#"{"Version":"2012-10-17","Statement":[]}"#.to_string(),
            })
            .await
            .expect("insert should succeed");

        let team_a = store
            .get_managed_policy("123456789012", "orders-readonly", "/team-a/")
            .await
            .expect("lookup should succeed")
            .expect("policy should exist");
        let team_b = store
            .get_managed_policy("123456789012", "orders-readonly", "/team-b/")
            .await
            .expect("lookup should succeed")
            .expect("policy should exist");

        assert_ne!(team_a.id, team_b.id);
        assert_eq!(team_a.policy_path.as_deref(), Some("/team-a/"));
        assert_eq!(team_b.policy_path.as_deref(), Some("/team-b/"));
    }

    #[tokio::test]
    async fn delete_managed_policy_removes_only_matching_path() {
        let (_temp_dir, pool) = test_pool().await;
        let store = SqliteManagedPolicyStore::new(&pool);

        store
            .insert_managed_policy(NewManagedPolicy {
                policy_id: "AIDAPOLICY00000011".to_string(),
                account_id: "123456789012".to_string(),
                policy_name: "orders-readonly".to_string(),
                policy_path: Some("/team-a/".to_string()),
                policy_document: r#"{"Version":"2012-10-17","Statement":[]}"#.to_string(),
            })
            .await
            .expect("insert should succeed");
        store
            .insert_managed_policy(NewManagedPolicy {
                policy_id: "AIDAPOLICY00000012".to_string(),
                account_id: "123456789012".to_string(),
                policy_name: "orders-readonly".to_string(),
                policy_path: Some("/team-b/".to_string()),
                policy_document: r#"{"Version":"2012-10-17","Statement":[]}"#.to_string(),
            })
            .await
            .expect("insert should succeed");

        store
            .delete_managed_policy("123456789012", "orders-readonly", "/team-a/")
            .await
            .expect("delete should succeed");

        let team_a = store
            .get_managed_policy("123456789012", "orders-readonly", "/team-a/")
            .await
            .expect("lookup should succeed");
        let team_b = store
            .get_managed_policy("123456789012", "orders-readonly", "/team-b/")
            .await
            .expect("lookup should succeed");
        assert!(team_a.is_none());
        assert!(team_b.is_some());
    }

    #[tokio::test]
    async fn attach_and_detach_policy_for_principal() {
        let (_temp_dir, pool) = test_pool().await;
        let policy_store = SqliteManagedPolicyStore::new(&pool);
        let principal_store = SqlitePrincipalStore::new(&pool);

        let principal = principal_store
            .create_principal(NewPrincipal {
                account_id: "123456789012".to_string(),
                kind: "user".to_string(),
                name: "alice".to_string(),
                path: "/".to_string(),
                user_id: "AIDATESTUSER000001".to_string(),
            })
            .await
            .expect("principal should be created");

        let policy = policy_store
            .insert_managed_policy(NewManagedPolicy {
                policy_id: "AIDAPOLICY00000021".to_string(),
                account_id: "123456789012".to_string(),
                policy_name: "orders-readonly".to_string(),
                policy_path: Some("/".to_string()),
                policy_document: r#"{"Version":"2012-10-17","Statement":[]}"#.to_string(),
            })
            .await
            .expect("insert should succeed");

        policy_store
            .attach_policy_to_principal(policy.id, principal.id)
            .await
            .expect("attach should succeed");
        let attached = policy_store
            .get_managed_policies_attached_to_principal(principal.id)
            .await
            .expect("attached policy lookup should succeed");
        assert_eq!(attached.len(), 1);

        policy_store
            .detach_policy_from_principal(policy.id, principal.id)
            .await
            .expect("detach should succeed");
        let detached = policy_store
            .get_managed_policies_attached_to_principal(principal.id)
            .await
            .expect("attached policy lookup should succeed");
        assert!(detached.is_empty());
    }
}
