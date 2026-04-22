use hiraeth_store::{
    StoreError,
    iam::{PrincipalInlinePolicy, PrincipalInlinePolicyStore},
};

#[derive(Debug, Clone)]
pub struct SqlitePrincipalInlinePolicyStore {
    pool: sqlx::SqlitePool,
}

impl SqlitePrincipalInlinePolicyStore {
    pub fn new(pool: &sqlx::SqlitePool) -> Self {
        Self { pool: pool.clone() }
    }
}

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
}
