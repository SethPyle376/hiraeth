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
