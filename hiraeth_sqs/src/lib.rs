use hiraeth_auth::ResolvedRequest;
use hiraeth_router::{Service, ServiceResponse};

pub struct SqsService {}

impl SqsService {
    pub fn new() -> Self {
        Self {}
    }
}

impl Service for SqsService {
    fn can_handle(&self, request: &ResolvedRequest) -> bool {
        true
    }

    fn handle_request(&self, request: ResolvedRequest) -> hiraeth_router::ServiceResponse {
        ServiceResponse {}
    }
}
