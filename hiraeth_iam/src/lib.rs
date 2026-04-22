use async_trait::async_trait;
use hiraeth_auth::AuthenticatedRequest;
use hiraeth_core::{
    ApiError, AuthContext, AuthMode, ResolvedRequest, ServiceResponse, auth::AuthorizationCheck,
};
use hiraeth_router::Service;
use hiraeth_store::{IamStore, StoreError, iam::PrincipalStore};

mod authorize;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AuthorizationMode {
    Enforce,
    Audit,
    Off,
}

#[derive(Debug, Clone)]
pub struct IamService<S: IamStore> {
    mode: AuthorizationMode,
    store: S,
}

impl<S: IamStore> IamService<S> {
    pub fn new(mode: AuthorizationMode, store: S) -> Self {
        Self { mode, store }
    }

    pub fn store(&self) -> &S {
        &self.store
    }

    pub async fn resolve_identity(
        &self,
        request: AuthenticatedRequest,
    ) -> Result<ResolvedRequest, ResolveIdentityError> {
        let principal = self
            .store
            .get_principal(request.auth_context.principal_id)
            .await
            .map_err(ResolveIdentityError::PrincipalStoreError)?
            .ok_or(ResolveIdentityError::PrincipalNotFound)?;

        Ok(ResolvedRequest {
            request: request.request,
            service: request.service,
            region: request.region,
            auth_context: AuthContext {
                access_key: request.auth_context.access_key,
                principal,
            },
            date: request.date,
        })
    }
}

impl From<AuthMode> for AuthorizationMode {
    fn from(value: AuthMode) -> Self {
        match value {
            AuthMode::Enforce => AuthorizationMode::Enforce,
            AuthMode::Audit => AuthorizationMode::Audit,
            AuthMode::Off => AuthorizationMode::Off,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResolveIdentityError {
    PrincipalStoreError(StoreError),
    PrincipalNotFound,
}

impl From<ResolveIdentityError> for ApiError {
    fn from(value: ResolveIdentityError) -> ApiError {
        match value {
            ResolveIdentityError::PrincipalStoreError(error) => {
                ApiError::InternalServerError(format!("Principal store error: {:?}", error))
            }
            ResolveIdentityError::PrincipalNotFound => {
                ApiError::NotAuthenticated("Principal not found for access key".to_string())
            }
        }
    }
}

#[async_trait]
impl<S> Service for IamService<S>
where
    S: IamStore + Send + Sync,
{
    fn can_handle(&self, _request: &ResolvedRequest) -> bool {
        false
    }

    async fn handle_request(
        &self,
        _request: ResolvedRequest,
    ) -> Result<ServiceResponse, hiraeth_core::ApiError> {
        todo!()
    }

    async fn resolve_authorization(
        &self,
        _request: &ResolvedRequest,
    ) -> Result<AuthorizationCheck, ServiceResponse> {
        todo!()
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use chrono::{TimeZone, Utc};
    use hiraeth_auth::{AuthContext as AuthenticatedAuthContext, AuthenticatedRequest};
    use hiraeth_http::IncomingRequest;
    use hiraeth_store::iam::{AccessKey, InMemoryIamStore, Principal};

    use super::{AuthorizationMode, IamService, ResolveIdentityError};

    fn authenticated_request(principal_id: i64) -> AuthenticatedRequest {
        AuthenticatedRequest {
            request: IncomingRequest {
                host: "sqs.us-east-1.amazonaws.com".to_string(),
                method: "POST".to_string(),
                path: "/".to_string(),
                query: None,
                headers: HashMap::new(),
                body: Vec::new(),
            },
            service: "sqs".to_string(),
            region: "us-east-1".to_string(),
            auth_context: AuthenticatedAuthContext {
                access_key: "AKIAIOSFODNN7EXAMPLE".to_string(),
                principal_id,
            },
            date: Utc.with_ymd_and_hms(2026, 4, 21, 12, 0, 0).unwrap(),
        }
    }

    fn access_key(principal_id: i64) -> AccessKey {
        AccessKey {
            key_id: "AKIAIOSFODNN7EXAMPLE".to_string(),
            principal_id,
            secret_key: "wJalrXUtnFEMI/K7MDENG/bPxRfiCYEXAMPLEKEY".to_string(),
            created_at: Utc
                .with_ymd_and_hms(2026, 4, 21, 12, 0, 0)
                .unwrap()
                .naive_utc(),
        }
    }

    fn principal(id: i64) -> Principal {
        Principal {
            id,
            account_id: "000000000000".to_string(),
            kind: "user".to_string(),
            name: "test".to_string(),
            created_at: Utc
                .with_ymd_and_hms(2026, 4, 21, 12, 0, 0)
                .unwrap()
                .naive_utc(),
        }
    }

    #[tokio::test]
    async fn resolve_identity_hydrates_principal_into_resolved_request() {
        let iam = IamService::new(
            AuthorizationMode::Audit,
            InMemoryIamStore::new([access_key(42)], [principal(42)]),
        );

        let resolved = iam
            .resolve_identity(authenticated_request(42))
            .await
            .expect("principal should resolve");

        assert_eq!(resolved.service, "sqs");
        assert_eq!(resolved.region, "us-east-1");
        assert_eq!(
            resolved.auth_context.access_key,
            "AKIAIOSFODNN7EXAMPLE".to_string()
        );
        assert_eq!(resolved.auth_context.principal.id, 42);
        assert_eq!(resolved.auth_context.principal.account_id, "000000000000");
        assert_eq!(resolved.auth_context.principal.name, "test");
    }

    #[tokio::test]
    async fn resolve_identity_returns_not_found_when_principal_is_missing() {
        let iam = IamService::new(
            AuthorizationMode::Audit,
            InMemoryIamStore::new([access_key(42)], []),
        );

        let error = iam
            .resolve_identity(authenticated_request(42))
            .await
            .expect_err("missing principal should fail identity resolution");

        assert_eq!(error, ResolveIdentityError::PrincipalNotFound);
    }
}
