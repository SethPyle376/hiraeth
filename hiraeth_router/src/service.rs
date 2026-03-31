use hiraeth_auth::ResolvedRequest;

use crate::ServiceResponse;

pub trait Service {
    fn can_handle(&self, request: &ResolvedRequest) -> bool;
    fn handle_request(&self, request: ResolvedRequest) -> ServiceResponse;
}
