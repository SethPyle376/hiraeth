use async_trait::async_trait;

mod access_key_store;
mod principal;
mod principal_inline_policy_store;

pub use access_key_store::{AccessKey, AccessKeyStore, InMemoryAccessKeyStore};
pub use principal::{InMemoryPrincipalStore, NewPrincipal, Principal, PrincipalStore};
pub use principal_inline_policy_store::{
    InMemoryPrincipalInlinePolicyStore, PrincipalInlinePolicy, PrincipalInlinePolicyStore,
};

pub struct InMemoryIamStore {
    pub access_key_store: InMemoryAccessKeyStore,
    pub principal_store: InMemoryPrincipalStore,
    pub principal_inline_policy_store: InMemoryPrincipalInlinePolicyStore,
}

impl InMemoryIamStore {
    pub fn new(
        access_keys: impl IntoIterator<Item = AccessKey>,
        principals: impl IntoIterator<Item = Principal>,
        principal_inline_policies: impl IntoIterator<Item = PrincipalInlinePolicy>,
    ) -> Self {
        Self {
            access_key_store: InMemoryAccessKeyStore::new(access_keys),
            principal_store: InMemoryPrincipalStore::new(principals),
            principal_inline_policy_store: InMemoryPrincipalInlinePolicyStore::new(
                principal_inline_policies,
            ),
        }
    }
}

impl Default for InMemoryIamStore {
    fn default() -> Self {
        Self::new([], [], [])
    }
}

#[async_trait]
impl AccessKeyStore for InMemoryIamStore {
    async fn get_secret_key(
        &self,
        access_key: &str,
    ) -> Result<Option<AccessKey>, crate::StoreError> {
        self.access_key_store.get_secret_key(access_key).await
    }

    async fn list_access_keys_for_principal(
        &self,
        principal_id: i64,
    ) -> Result<Vec<AccessKey>, crate::StoreError> {
        self.access_key_store
            .list_access_keys_for_principal(principal_id)
            .await
    }

    async fn insert_secret_key(
        &self,
        access_key: &str,
        secret_key: &str,
        principal_id: i64,
    ) -> Result<AccessKey, crate::StoreError> {
        self.access_key_store
            .insert_secret_key(access_key, secret_key, principal_id)
            .await
    }
}

#[async_trait]
impl PrincipalStore for InMemoryIamStore {
    async fn get_principal(
        &self,
        principal_id: i64,
    ) -> Result<Option<Principal>, crate::StoreError> {
        self.principal_store.get_principal(principal_id).await
    }

    async fn get_principal_by_identity(
        &self,
        account_id: &str,
        kind: &str,
        name: &str,
    ) -> Result<Option<Principal>, crate::StoreError> {
        self.principal_store
            .get_principal_by_identity(account_id, kind, name)
            .await
    }

    async fn list_principals(&self) -> Result<Vec<Principal>, crate::StoreError> {
        self.principal_store.list_principals().await
    }

    async fn create_principal(
        &self,
        principal: NewPrincipal,
    ) -> Result<Principal, crate::StoreError> {
        self.principal_store.create_principal(principal).await
    }
}

#[async_trait]
impl principal_inline_policy_store::PrincipalInlinePolicyStore for InMemoryIamStore {
    async fn get_inline_policies_for_principal(
        &self,
        principal_id: i64,
    ) -> Result<Vec<PrincipalInlinePolicy>, crate::StoreError> {
        self.principal_inline_policy_store
            .get_inline_policies_for_principal(principal_id)
            .await
    }
}

pub trait IamStore: AccessKeyStore + PrincipalStore + PrincipalInlinePolicyStore {}

impl<T> IamStore for T where T: AccessKeyStore + PrincipalStore + PrincipalInlinePolicyStore {}
