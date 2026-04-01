use hiraeth_auth::ResolvedRequest;
use hiraeth_core::ApiError;
use hiraeth_router::{Service, ServiceResponse};

enum SqsError {
    QueueNotFound,
}

impl Into<ApiError> for SqsError {
    fn into(self) -> ApiError {
        match self {
            SqsError::QueueNotFound => ApiError::NotFound,
        }
    }
}

pub struct SqsService {}

impl SqsService {
    pub fn new() -> Self {
        Self {}
    }
}

impl Service for SqsService {
    fn can_handle(&self, request: &ResolvedRequest) -> bool {
        request.service == "sqs"
    }

    fn handle_request(
        &self,
        request: ResolvedRequest,
    ) -> Result<ServiceResponse, hiraeth_core::ApiError> {
        Ok(ServiceResponse {})
    }
}
