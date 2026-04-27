use async_trait::async_trait;

mod access_key_store;
mod managed_policy_store;
mod principal;
mod principal_inline_policy_store;

pub use access_key_store::{AccessKey, AccessKeyStore, InMemoryAccessKeyStore};
pub use managed_policy_store::{
    InMemoryManagedPolicyStore, ManagedPolicy, ManagedPolicyStore, NewManagedPolicy,
};
pub use principal::{InMemoryPrincipalStore, NewPrincipal, Principal, PrincipalStore};
pub use principal_inline_policy_store::{
    InMemoryPrincipalInlinePolicyStore, PrincipalInlinePolicy, PrincipalInlinePolicyStore,
};

use crate::iam::managed_policy_store::ManagedPolicyPrincipalAttachment;

pub struct InMemoryIamStore {
    pub access_key_store: InMemoryAccessKeyStore,
    pub principal_store: InMemoryPrincipalStore,
    pub principal_inline_policy_store: InMemoryPrincipalInlinePolicyStore,
    pub managed_policy_store: InMemoryManagedPolicyStore,
}

impl InMemoryIamStore {
    pub fn new(
        access_keys: impl IntoIterator<Item = AccessKey>,
        principals: impl IntoIterator<Item = Principal>,
        principal_inline_policies: impl IntoIterator<Item = PrincipalInlinePolicy>,
        managed_policies: impl IntoIterator<Item = ManagedPolicy>,
        managed_policy_attachments: impl IntoIterator<Item = ManagedPolicyPrincipalAttachment>,
    ) -> Self {
        Self {
            access_key_store: InMemoryAccessKeyStore::new(access_keys),
            principal_store: InMemoryPrincipalStore::new(principals),
            principal_inline_policy_store: InMemoryPrincipalInlinePolicyStore::new(
                principal_inline_policies,
            ),
            managed_policy_store: InMemoryManagedPolicyStore::new(
                managed_policies,
                managed_policy_attachments,
            ),
        }
    }
}

impl Default for InMemoryIamStore {
    fn default() -> Self {
        Self::new([], [], [], [], [])
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

    async fn delete_access_key_for_principal(
        &self,
        principal_id: i64,
        access_key: &str,
    ) -> Result<(), crate::StoreError> {
        self.access_key_store
            .delete_access_key_for_principal(principal_id, access_key)
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

    async fn delete_principal(&self, principal_id: i64) -> Result<(), crate::StoreError> {
        self.principal_store.delete_principal(principal_id).await
    }

    async fn delete_user(&self, account_id: &str, name: &str) -> Result<(), crate::StoreError> {
        self.principal_store.delete_user(account_id, name).await
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

    async fn put_inline_policy(
        &self,
        principal_id: i64,
        policy_name: &str,
        policy_document: &str,
    ) -> Result<PrincipalInlinePolicy, crate::StoreError> {
        self.principal_inline_policy_store
            .put_inline_policy(principal_id, policy_name, policy_document)
            .await
    }

    async fn delete_inline_policy(
        &self,
        principal_id: i64,
        policy_name: &str,
    ) -> Result<(), crate::StoreError> {
        self.principal_inline_policy_store
            .delete_inline_policy(principal_id, policy_name)
            .await
    }
}

#[async_trait]
impl ManagedPolicyStore for InMemoryIamStore {
    async fn insert_managed_policy(
        &self,
        policy: NewManagedPolicy,
    ) -> Result<ManagedPolicy, crate::StoreError> {
        self.managed_policy_store
            .insert_managed_policy(policy)
            .await
    }

    async fn get_managed_policy(
        &self,
        account_id: &str,
        policy_name: &str,
    ) -> Result<Option<ManagedPolicy>, crate::StoreError> {
        self.managed_policy_store
            .get_managed_policy(account_id, policy_name)
            .await
    }

    async fn attach_policy_to_principal(
        &self,
        policy_id: i64,
        principal_id: i64,
    ) -> Result<(), crate::StoreError> {
        self.managed_policy_store
            .attach_policy_to_principal(policy_id, principal_id)
            .await
    }

    async fn detach_policy_from_principal(
        &self,
        policy_id: i64,
        principal_id: i64,
    ) -> Result<(), crate::StoreError> {
        self.managed_policy_store
            .detach_policy_from_principal(policy_id, principal_id)
            .await
    }

    async fn delete_managed_policy(
        &self,
        account_id: &str,
        policy_name: &str,
    ) -> Result<(), crate::StoreError> {
        self.managed_policy_store
            .delete_managed_policy(account_id, policy_name)
            .await
    }

    async fn get_managed_policies_attached_to_principal(
        &self,
        principal_id: i64,
    ) -> Result<Vec<ManagedPolicy>, crate::StoreError> {
        self.managed_policy_store
            .get_managed_policies_attached_to_principal(principal_id)
            .await
    }
}

pub trait IamStore:
    AccessKeyStore + PrincipalStore + PrincipalInlinePolicyStore + ManagedPolicyStore
{
}

impl<T> IamStore for T where
    T: AccessKeyStore + PrincipalStore + PrincipalInlinePolicyStore + ManagedPolicyStore
{
}
