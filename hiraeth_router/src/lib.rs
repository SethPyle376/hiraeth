mod service;

use hiraeth_auth::ResolvedRequest;
use hiraeth_core::ApiError;
pub use service::Service;

pub struct ServiceResponse {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ServiceRouterError {
    NoServiceFound,
}

impl Into<ApiError> for ServiceRouterError {
    fn into(self) -> ApiError {
        match self {
            ServiceRouterError::NoServiceFound => ApiError::NotFound,
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
    pub fn route(&self, request: ResolvedRequest) -> Result<ServiceResponse, ApiError> {
        let service = self
            .services
            .iter()
            .find(|s| s.can_handle(&request))
            .ok_or(ServiceRouterError::NoServiceFound.into())?;

        service.handle_request(request)
    }

    pub fn register_service(&mut self, service: Box<dyn Service + Send + Sync>) {
        self.services.push(service);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn it_works() {}
}
