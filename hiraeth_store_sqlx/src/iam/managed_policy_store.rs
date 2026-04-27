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
                INSERT INTO iam_managed_policies (account_id, policy_name, policy_path, policy_document, created_at, updated_at)
                VALUES (?, ?, ?, ?, CURRENT_TIMESTAMP, CURRENT_TIMESTAMP)
                RETURNING id, account_id, policy_name, policy_path, policy_document, created_at, updated_at
                "#,
                policy.account_id,
                policy.policy_name,
                policy.policy_path,
                policy.policy_document
            )
            .fetch_one(&self.pool)
            .await
            .map_err(map_sqlx_error)
    }
}
