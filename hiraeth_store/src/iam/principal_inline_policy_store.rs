use crate::StoreError;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrincipalInlinePolicy {
    pub id: i64,
    pub principal_id: i64,
    pub policy_name: String,
    pub policy_document: String,
    pub created_at: chrono::NaiveDateTime,
    pub updated_at: chrono::NaiveDateTime,
}

#[allow(async_fn_in_trait)]
pub trait PrincipalInlinePolicyStore {
    async fn get_inline_policies_for_principal(
        &self,
        principal_id: i64,
    ) -> Result<Vec<PrincipalInlinePolicy>, StoreError>;
}

pub struct InMemoryPrincipalInlinePolicyStore {
    policies: Vec<PrincipalInlinePolicy>,
}

impl InMemoryPrincipalInlinePolicyStore {
    pub fn new(policies: impl IntoIterator<Item = PrincipalInlinePolicy>) -> Self {
        Self {
            policies: policies.into_iter().collect(),
        }
    }
}

impl PrincipalInlinePolicyStore for InMemoryPrincipalInlinePolicyStore {
    async fn get_inline_policies_for_principal(
        &self,
        principal_id: i64,
    ) -> Result<Vec<PrincipalInlinePolicy>, StoreError> {
        Ok(self
            .policies
            .iter()
            .filter(|p| p.principal_id == principal_id)
            .cloned()
            .collect())
    }
}
