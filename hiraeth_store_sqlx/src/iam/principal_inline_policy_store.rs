use async_trait::async_trait;
use hiraeth_store::{
    StoreError,
    iam::{PrincipalInlinePolicy, PrincipalInlinePolicyStore},
};
use sqlx::Row;

#[derive(Debug, Clone)]
pub struct SqlitePrincipalInlinePolicyStore {
    pool: sqlx::SqlitePool,
}

impl SqlitePrincipalInlinePolicyStore {
    pub fn new(pool: &sqlx::SqlitePool) -> Self {
        Self { pool: pool.clone() }
    }
}

#[async_trait]
impl PrincipalInlinePolicyStore for SqlitePrincipalInlinePolicyStore {
    async fn get_inline_policies_for_principal(
        &self,
        principal_id: i64,
    ) -> Result<Vec<PrincipalInlinePolicy>, StoreError> {
        sqlx::query_as!(
            PrincipalInlinePolicy,
            r#"
            SELECT *
            FROM iam_principal_inline_policies
            WHERE principal_id = ?
            "#,
            principal_id
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|err| hiraeth_store::StoreError::StorageFailure(err.to_string()))
    }

    async fn put_inline_policy(
        &self,
        principal_id: i64,
        policy_name: &str,
        policy_document: &str,
    ) -> Result<PrincipalInlinePolicy, StoreError> {
        let row = sqlx::query(
            r#"
            INSERT INTO iam_principal_inline_policies (principal_id, policy_name, policy_document)
            VALUES (?, ?, ?)
            ON CONFLICT(principal_id, policy_name) DO UPDATE SET
              policy_document = excluded.policy_document,
              updated_at = CURRENT_TIMESTAMP
            RETURNING id, principal_id, policy_name, policy_document, created_at, updated_at
            "#,
        )
        .bind(principal_id)
        .bind(policy_name)
        .bind(policy_document)
        .fetch_one(&self.pool)
        .await
        .map_err(|err| hiraeth_store::StoreError::StorageFailure(err.to_string()))?;

        Ok(PrincipalInlinePolicy {
            id: row.try_get("id").map_err(map_sqlx_error)?,
            principal_id: row.try_get("principal_id").map_err(map_sqlx_error)?,
            policy_name: row.try_get("policy_name").map_err(map_sqlx_error)?,
            policy_document: row.try_get("policy_document").map_err(map_sqlx_error)?,
            created_at: row.try_get("created_at").map_err(map_sqlx_error)?,
            updated_at: row.try_get("updated_at").map_err(map_sqlx_error)?,
        })
    }

    async fn delete_inline_policy(
        &self,
        principal_id: i64,
        policy_name: &str,
    ) -> Result<(), StoreError> {
        let result = sqlx::query(
            "DELETE FROM iam_principal_inline_policies WHERE principal_id = ? AND policy_name = ?",
        )
        .bind(principal_id)
        .bind(policy_name)
        .execute(&self.pool)
        .await
        .map_err(|err| hiraeth_store::StoreError::StorageFailure(err.to_string()))?;

        if result.rows_affected() == 0 {
            return Err(StoreError::NotFound(format!(
                "Inline policy {policy_name} not found for principal {principal_id}"
            )));
        }

        Ok(())
    }
}

fn map_sqlx_error(err: sqlx::Error) -> StoreError {
    StoreError::StorageFailure(err.to_string())
}
