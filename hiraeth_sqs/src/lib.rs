use async_trait::async_trait;
use hiraeth_auth::ResolvedRequest;
use hiraeth_core::ApiError;
use hiraeth_router::{Service, ServiceResponse};
use hiraeth_store::sqs::{SqsStore, SqsStoreError};

mod queue;

#[derive(Debug, Clone, PartialEq, Eq)]
enum SqsError {
    QueueNotFound,
    StoreError(SqsStoreError),
    BadRequest(String)
}

impl From<SqsError> for ApiError {
    fn from(value: SqsError) -> ApiError {
        match value {
            SqsError::QueueNotFound => ApiError::NotFound("Queue not found".to_string()),
            SqsError::StoreError(sqs_store_error) => {
                ApiError::InternalServerError(format!("SQS store error: {:?}", sqs_store_error))
            },
            SqsError::BadRequest(error) => {
                ApiError::BadRequest(format!("SQS Bad Request: {:?}", error))
            }
        }
    }
}

pub struct SqsService<S: SqsStore> {
    store: S,
}

impl<S: SqsStore> SqsService<S> {
    pub fn new(store: S) -> Self {
        Self { store }
    }
}

#[async_trait]
impl<S> Service for SqsService<S>
where
    S: SqsStore + Send + Sync + 'static,
{
    fn can_handle(&self, request: &ResolvedRequest) -> bool {
        request.service == "sqs"
    }

    async fn handle_request(
        &self,
        request: ResolvedRequest,
    ) -> Result<ServiceResponse, hiraeth_core::ApiError> {
        match request.request.headers.get("x-amz-target") {
            Some(target) => match target.as_str() {
                "AmazonSQS.CreateQueue" => queue::create_queue(&request, &self.store)
                    .await
                    .map_err(Into::into),
                "AmazonSQS.SendMessage" => {
                    todo!()
                }
                _ => {
                    return Err(ApiError::NotFound(format!(
                        "Unknown SQS action: {}",
                        target
                    )));
                }
            },
            _ => Err(ApiError::NotFound(
                "Missing x-amz-target header".to_string(),
            )),
        }
    }
}
