mod service;

use hiraeth_auth::ResolvedRequest;
use hiraeth_core::ApiError;
pub use service::Service;

pub struct ServiceResponse {
    pub status_code: u16,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

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
    services: Vec<Box<dyn Service + Send + Sync>>,
}

impl Default for ServiceRouter {
    fn default() -> Self {
        Self {
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

        service.handle_request(request).await
    }

    pub fn register_service(&mut self, service: Box<dyn Service + Send + Sync>) {
        self.services.push(service);
    }
}
