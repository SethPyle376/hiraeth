mod resolved_request;
mod service;

pub use hiraeth_http::IncomingRequest;

use hiraeth_http::ServiceResponse;
pub use service::Service;

pub struct ServiceRouter {}

impl ServiceRouter {
    pub fn route(&self, request: IncomingRequest) -> ServiceResponse {
        todo!()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn it_works() {}
}
