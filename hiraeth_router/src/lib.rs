mod service;

use hiraeth_auth::{AuthorizationResult, Authorizer, ResolvedRequest};
use hiraeth_core::ApiError;
pub use hiraeth_core::ServiceResponse;
pub use service::Service;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ServiceRouterError {
    NoServiceFound,
}

impl From<ServiceRouterError> for ApiError {
    fn from(value: ServiceRouterError) -> ApiError {
        match value {
            ServiceRouterError::NoServiceFound => {
                ApiError::NotFound("No service found to handle the request".to_string())
            }
        }
    }
}

pub struct ServiceRouter {
    authorizer: Box<dyn Authorizer + Send + Sync>,
    services: Vec<Box<dyn Service + Send + Sync>>,
}

impl ServiceRouter {
    pub fn new(authorizer: Box<dyn Authorizer + Send + Sync>) -> Self {
        Self {
            authorizer,
            services: Vec::new(),
        }
    }
}

impl ServiceRouter {
    pub async fn route(&self, request: ResolvedRequest) -> Result<ServiceResponse, ApiError> {
        let service = self
            .services
            .iter()
            .find(|s| s.can_handle(&request))
            .ok_or(ApiError::from(ServiceRouterError::NoServiceFound))?;

        let check = match service.resolve_authorization(&request).await {
            Ok(check) => check,
            Err(response) => return Ok(response),
        };

        let Some(principal) = self.authorizer.get_principal(&request).await else {
            return Ok(self.authorizer.unauthorized_response());
        };

        let auth_result = self.authorizer.authorize_request(&check, &principal).await;

        match auth_result {
            AuthorizationResult::Allow => service.handle_request(request).await,
            AuthorizationResult::Deny => Ok(self.authorizer.unauthorized_response()),
        }
    }

    pub fn register_service(&mut self, service: Box<dyn Service + Send + Sync>) {
        self.services.push(service);
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use async_trait::async_trait;
    use chrono::{TimeZone, Utc};
    use hiraeth_auth::{AuthContext, AuthorizationResult, Authorizer, ResolvedRequest};
    use hiraeth_core::{
        ApiError, ServiceResponse,
        auth::{AuthorizationCheck, PolicyPrincipal},
    };
    use hiraeth_http::IncomingRequest;
    use hiraeth_store::principal::Principal;

    use super::{Service, ServiceRouter};

    fn resolved_request() -> ResolvedRequest {
        ResolvedRequest {
            request: IncomingRequest {
                host: "sqs.us-east-1.amazonaws.com".to_string(),
                method: "POST".to_string(),
                path: "/".to_string(),
                query: None,
                headers: HashMap::new(),
                body: Vec::new(),
            },
            service: "test".to_string(),
            region: "us-east-1".to_string(),
            auth_context: AuthContext {
                access_key: "AKIAIOSFODNN7EXAMPLE".to_string(),
                principal: Principal {
                    id: 1,
                    account_id: "123456789012".to_string(),
                    kind: "user".to_string(),
                    name: "test-user".to_string(),
                    created_at: Utc
                        .with_ymd_and_hms(2026, 4, 20, 12, 0, 0)
                        .unwrap()
                        .naive_utc(),
                },
            },
            date: Utc.with_ymd_and_hms(2026, 4, 20, 12, 0, 0).unwrap(),
        }
    }

    struct AuthorizedService;

    #[async_trait]
    impl Service for AuthorizedService {
        fn can_handle(&self, request: &ResolvedRequest) -> bool {
            request.service == "test"
        }

        async fn handle_request(
            &self,
            _request: ResolvedRequest,
        ) -> Result<ServiceResponse, ApiError> {
            Ok(ServiceResponse {
                status_code: 200,
                body: Vec::new(),
                headers: Vec::new(),
            })
        }

        async fn resolve_authorization(
            &self,
            _request: &ResolvedRequest,
        ) -> Result<AuthorizationCheck, ServiceResponse> {
            Ok(AuthorizationCheck {
                action: "test:Action".to_string(),
                resource: "*".to_string(),
                resource_policy: None,
            })
        }
    }

    struct AuthorizationErrorService;

    #[async_trait]
    impl Service for AuthorizationErrorService {
        fn can_handle(&self, request: &ResolvedRequest) -> bool {
            request.service == "test"
        }

        async fn handle_request(
            &self,
            _request: ResolvedRequest,
        ) -> Result<ServiceResponse, ApiError> {
            panic!("service should not execute when authorization resolution fails");
        }

        async fn resolve_authorization(
            &self,
            _request: &ResolvedRequest,
        ) -> Result<AuthorizationCheck, ServiceResponse> {
            Err(ServiceResponse {
                status_code: 418,
                body: Vec::new(),
                headers: Vec::new(),
            })
        }
    }

    struct MissingPrincipalAuthorizer;

    #[async_trait]
    impl Authorizer for MissingPrincipalAuthorizer {
        async fn get_principal(&self, _request: &ResolvedRequest) -> Option<PolicyPrincipal> {
            None
        }

        async fn authorize_request(
            &self,
            _check: &AuthorizationCheck,
            _principal: &PolicyPrincipal,
        ) -> AuthorizationResult {
            panic!("authorization should not run without a principal");
        }

        fn unauthorized_response(&self) -> ServiceResponse {
            ServiceResponse {
                status_code: 403,
                body: Vec::new(),
                headers: Vec::new(),
            }
        }
    }

    #[tokio::test]
    async fn route_returns_service_response_when_authorization_resolution_fails() {
        let mut router = ServiceRouter::new(Box::new(MissingPrincipalAuthorizer));
        router.register_service(Box::new(AuthorizationErrorService));

        let response = router
            .route(resolved_request())
            .await
            .expect("router should return service authorization response");

        assert_eq!(response.status_code, 418);
    }

    #[tokio::test]
    async fn route_returns_unauthorized_response_when_principal_cannot_be_resolved() {
        let mut router = ServiceRouter::new(Box::new(MissingPrincipalAuthorizer));
        router.register_service(Box::new(AuthorizedService));

        let response = router
            .route(resolved_request())
            .await
            .expect("router should return unauthorized response");

        assert_eq!(response.status_code, 403);
    }
}
