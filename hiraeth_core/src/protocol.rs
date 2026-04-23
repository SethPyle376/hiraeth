use std::{
    collections::BTreeMap,
    fmt::{Debug, Display},
};

use hiraeth_http::IncomingRequest;
use quick_xml::se::to_string as to_xml_string;
use serde::{Serialize, de::DeserializeOwned};
use url::form_urlencoded;

use crate::ApiError;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServiceResponse {
    pub status_code: u16,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AwsErrorFault {
    Sender,
    Server,
}

impl AwsErrorFault {
    pub fn as_query_type(self) -> &'static str {
        match self {
            AwsErrorFault::Sender => "Sender",
            AwsErrorFault::Server => "Server",
        }
    }

    pub fn sender_fault(self) -> bool {
        matches!(self, AwsErrorFault::Sender)
    }
}

pub trait AwsServiceError: Display {
    fn status_code(&self) -> u16;

    fn namespace(&self) -> &'static str;

    fn code(&self) -> &'static str;

    fn query_error_prefix(&self) -> &'static str;

    fn fault(&self) -> AwsErrorFault;

    fn query_code(&self) -> &'static str {
        self.code()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct AwsJsonErrorBody {
    #[serde(rename = "__type")]
    error_type: String,
    message: String,
}

pub fn render_aws_json_error<E: AwsServiceError>(error: &E) -> ServiceResponse {
    let body = AwsJsonErrorBody {
        error_type: format!("{}#{}", error.namespace(), error.code()),
        message: error.to_string(),
    };

    ServiceResponse {
        status_code: error.status_code(),
        headers: vec![
            (
                "content-type".to_string(),
                "application/x-amz-json-1.0".to_string(),
            ),
            (
                "x-amzn-query-error".to_string(),
                format!(
                    "{}.{};{}",
                    error.query_error_prefix(),
                    error.query_code(),
                    error.fault().as_query_type()
                ),
            ),
        ],
        body: serde_json::to_vec(&body).unwrap_or_else(|_| vec![]),
    }
}

pub fn aws_batch_error_details<E: AwsServiceError>(error: &E) -> (&'static str, bool) {
    (error.code(), error.fault().sender_fault())
}

pub fn empty_response() -> ServiceResponse {
    ServiceResponse {
        status_code: 200,
        headers: vec![],
        body: vec![],
    }
}

pub fn json_response<T: Serialize>(
    body: &T,
) -> Result<ServiceResponse, ResponseSerializationError> {
    Ok(ServiceResponse {
        status_code: 200,
        headers: vec![],
        body: json_body(body)?,
    })
}

pub fn json_body<T: Serialize>(body: &T) -> Result<Vec<u8>, ResponseSerializationError> {
    serde_json::to_vec(body).map_err(ResponseSerializationError::json)
}

pub fn xml_response<T: Serialize>(body: &T) -> Result<ServiceResponse, ResponseSerializationError> {
    Ok(ServiceResponse {
        status_code: 200,
        headers: vec![(
            "content-type".to_string(),
            "text/xml; charset=utf-8".to_string(),
        )],
        body: xml_body(body)?,
    })
}

pub fn xml_body<T: Serialize>(body: &T) -> Result<Vec<u8>, ResponseSerializationError> {
    to_xml_string(body)
        .map(String::into_bytes)
        .map_err(ResponseSerializationError::xml)
}

pub fn parse_json_body<T: DeserializeOwned>(body: &[u8]) -> Result<T, RequestBodyParseError> {
    serde_json::from_slice(body).map_err(RequestBodyParseError::new)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AwsQueryParams {
    encoded: Vec<u8>,
    values: BTreeMap<String, Vec<String>>,
}

impl AwsQueryParams {
    pub fn parse(request: &IncomingRequest) -> Result<Self, AwsQueryParseError> {
        let encoded = encoded_aws_query_request(request)?;
        Ok(Self::from_encoded(encoded))
    }

    fn from_encoded(encoded: Vec<u8>) -> Self {
        let values =
            form_urlencoded::parse(&encoded).fold(BTreeMap::new(), |mut values, (name, value)| {
                values
                    .entry(name.into_owned())
                    .or_insert_with(Vec::new)
                    .push(value.into_owned());
                values
            });

        Self { encoded, values }
    }

    pub fn is_empty(&self) -> bool {
        self.values.is_empty()
    }

    pub fn contains(&self, key: &str) -> bool {
        self.values.contains_key(key)
    }

    pub fn get(&self, key: &str) -> Option<&str> {
        self.values.get(key)?.first().map(String::as_str)
    }

    pub fn get_all(&self, key: &str) -> Option<&[String]> {
        self.values.get(key).map(Vec::as_slice)
    }

    pub fn deserialize<T: DeserializeOwned>(&self) -> Result<T, AwsQueryParseError> {
        serde_urlencoded::from_bytes(&self.encoded).map_err(AwsQueryParseError::new)
    }
}

pub fn parse_aws_query_params(
    request: &IncomingRequest,
) -> Result<AwsQueryParams, AwsQueryParseError> {
    AwsQueryParams::parse(request)
}

pub fn parse_aws_query_request<T: DeserializeOwned>(
    request: &IncomingRequest,
) -> Result<T, AwsQueryParseError> {
    parse_aws_query_params(request)?.deserialize()
}

pub fn render_result<T, E>(result: Result<T, E>) -> ServiceResponse
where
    T: Into<ServiceResponse>,
    E: Into<ServiceResponse>,
{
    match result {
        Ok(success) => success.into(),
        Err(error) => error.into(),
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResponseSerializationError {
    message: String,
}

impl ResponseSerializationError {
    fn json(error: serde_json::Error) -> Self {
        Self {
            message: format!("failed to serialize response: {}", error),
        }
    }

    fn xml(error: impl Display) -> Self {
        Self {
            message: format!("failed to serialize response: {}", error),
        }
    }
}

impl Display for ResponseSerializationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl std::error::Error for ResponseSerializationError {}

impl From<ResponseSerializationError> for ApiError {
    fn from(value: ResponseSerializationError) -> Self {
        ApiError::InternalServerError(value.to_string())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RequestBodyParseError {
    message: String,
}

impl RequestBodyParseError {
    fn new(error: serde_json::Error) -> Self {
        Self {
            message: format!("Invalid request body: {}", error),
        }
    }
}

impl Display for RequestBodyParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl std::error::Error for RequestBodyParseError {}

impl From<RequestBodyParseError> for ApiError {
    fn from(value: RequestBodyParseError) -> Self {
        ApiError::BadRequest(value.to_string())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AwsQueryParseError {
    message: String,
}

impl AwsQueryParseError {
    fn new(error: serde_urlencoded::de::Error) -> Self {
        Self {
            message: format!("Invalid AWS query request: {}", error),
        }
    }

    fn unsupported_content_type(content_type: &str) -> Self {
        Self {
            message: format!(
                "Invalid AWS query request: unsupported content-type '{}'",
                content_type
            ),
        }
    }
}

impl Display for AwsQueryParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl std::error::Error for AwsQueryParseError {}

impl From<AwsQueryParseError> for ApiError {
    fn from(value: AwsQueryParseError) -> Self {
        ApiError::BadRequest(value.to_string())
    }
}

fn encoded_aws_query_request(request: &IncomingRequest) -> Result<Vec<u8>, AwsQueryParseError> {
    let mut encoded = Vec::new();

    if let Some(query) = request.query.as_deref().filter(|query| !query.is_empty()) {
        encoded.extend_from_slice(query.as_bytes());
    }

    if !request.body.is_empty() {
        let content_type = request
            .headers
            .get("content-type")
            .map(String::as_str)
            .unwrap_or("");

        if !content_type.is_empty() && !is_form_urlencoded_content_type(content_type) {
            return Err(AwsQueryParseError::unsupported_content_type(content_type));
        }

        if !encoded.is_empty() {
            encoded.push(b'&');
        }

        encoded.extend_from_slice(&request.body);
    }

    Ok(encoded)
}

fn is_form_urlencoded_content_type(content_type: &str) -> bool {
    content_type
        .split(';')
        .next()
        .map(str::trim)
        .is_some_and(|value| value.eq_ignore_ascii_case("application/x-www-form-urlencoded"))
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use hiraeth_http::IncomingRequest;
    use serde::{Deserialize, Serialize};

    use super::{
        AwsErrorFault, AwsQueryParams, AwsServiceError, ServiceResponse, aws_batch_error_details,
        empty_response, json_response, parse_aws_query_params, parse_aws_query_request,
        parse_json_body, render_aws_json_error, render_result, xml_response,
    };

    #[derive(Debug)]
    struct TestError;

    impl std::fmt::Display for TestError {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write!(f, "test message")
        }
    }

    impl AwsServiceError for TestError {
        fn status_code(&self) -> u16 {
            400
        }

        fn namespace(&self) -> &'static str {
            "com.amazonaws.test"
        }

        fn code(&self) -> &'static str {
            "TestError"
        }

        fn query_error_prefix(&self) -> &'static str {
            "AWS.TestService"
        }

        fn fault(&self) -> AwsErrorFault {
            AwsErrorFault::Sender
        }

        fn query_code(&self) -> &'static str {
            "DifferentQueryError"
        }
    }

    impl From<TestError> for ServiceResponse {
        fn from(value: TestError) -> Self {
            render_aws_json_error(&value)
        }
    }

    #[test]
    fn renders_aws_json_error_response() {
        let response = render_aws_json_error(&TestError);
        let body: serde_json::Value =
            serde_json::from_slice(&response.body).expect("error response should be json");

        assert_eq!(response.status_code, 400);
        assert_eq!(
            response
                .headers
                .iter()
                .find(|(name, _)| name == "x-amzn-query-error")
                .map(|(_, value)| value.as_str()),
            Some("AWS.TestService.DifferentQueryError;Sender")
        );
        assert_eq!(body["__type"], "com.amazonaws.test#TestError");
        assert_eq!(body["message"], "test message");
    }

    #[test]
    fn exposes_batch_error_details_from_service_error_metadata() {
        assert_eq!(aws_batch_error_details(&TestError), ("TestError", true));
    }

    #[test]
    fn builds_empty_success_response() {
        let response = empty_response();

        assert_eq!(response.status_code, 200);
        assert!(response.headers.is_empty());
        assert!(response.body.is_empty());
    }

    #[test]
    fn builds_json_success_response() {
        let response = json_response(&serde_json::json!({"ok": true}))
            .expect("json response should serialize");
        let body: serde_json::Value =
            serde_json::from_slice(&response.body).expect("response body should be json");

        assert_eq!(response.status_code, 200);
        assert_eq!(body["ok"], true);
    }

    #[derive(Debug, Serialize)]
    #[serde(rename = "ExampleResponse")]
    struct ExampleXmlResponse {
        #[serde(rename = "@xmlns")]
        xmlns: &'static str,
        #[serde(rename = "Message")]
        message: &'static str,
    }

    #[test]
    fn builds_xml_success_response() {
        let response = xml_response(&ExampleXmlResponse {
            xmlns: "urn:test",
            message: "hello",
        })
        .expect("xml response should serialize");
        let body = String::from_utf8(response.body).expect("response body should be utf-8");

        assert_eq!(response.status_code, 200);
        assert_eq!(
            response.headers,
            vec![(
                "content-type".to_string(),
                "text/xml; charset=utf-8".to_string()
            )]
        );
        assert_eq!(
            body,
            r#"<ExampleResponse xmlns="urn:test"><Message>hello</Message></ExampleResponse>"#
        );
    }

    #[test]
    fn parses_json_body_with_consistent_error_message() {
        let error = parse_json_body::<serde_json::Value>(b"{").expect_err("body should fail");

        assert!(error.to_string().starts_with("Invalid request body:"));
    }

    #[test]
    fn renders_result_with_success_or_error_response() {
        let success = render_result::<_, TestError>(Ok(empty_response()));
        let failure = render_result::<ServiceResponse, _>(Err(TestError));

        assert_eq!(success.status_code, 200);
        assert_eq!(failure.status_code, 400);
    }

    fn request_with_query_and_body(
        query: Option<&str>,
        content_type: Option<&str>,
        body: &[u8],
    ) -> IncomingRequest {
        let mut headers = HashMap::new();
        if let Some(content_type) = content_type {
            headers.insert("content-type".to_string(), content_type.to_string());
        }

        IncomingRequest {
            host: "iam.amazonaws.com".to_string(),
            method: "POST".to_string(),
            path: "/".to_string(),
            query: query.map(str::to_string),
            headers,
            body: body.to_vec(),
        }
    }

    #[derive(Debug, Deserialize, PartialEq, Eq)]
    struct QueryRequest {
        #[serde(rename = "Action")]
        action: String,
        #[serde(rename = "Version")]
        version: String,
        #[serde(rename = "UserName")]
        user_name: String,
        #[serde(rename = "Path")]
        path: String,
    }

    #[test]
    fn parse_aws_query_request_reads_form_body_into_struct() {
        let request = request_with_query_and_body(
            None,
            Some("application/x-www-form-urlencoded"),
            b"Action=CreateUser&Version=2010-05-08&UserName=test-user&Path=%2Fengineering%2F",
        );

        let parsed: QueryRequest =
            parse_aws_query_request(&request).expect("form body should deserialize");

        assert_eq!(
            parsed,
            QueryRequest {
                action: "CreateUser".to_string(),
                version: "2010-05-08".to_string(),
                user_name: "test-user".to_string(),
                path: "/engineering/".to_string(),
            }
        );
    }

    #[test]
    fn parse_aws_query_params_merges_query_string_and_body() {
        let request = request_with_query_and_body(
            Some("Action=CreateUser&Version=2010-05-08"),
            Some("application/x-www-form-urlencoded; charset=utf-8"),
            b"UserName=test-user&Path=%2F",
        );

        let params = parse_aws_query_params(&request).expect("query params should parse");

        assert_eq!(params.get("Action"), Some("CreateUser"));
        assert_eq!(params.get("Version"), Some("2010-05-08"));
        assert_eq!(params.get("UserName"), Some("test-user"));
        assert_eq!(params.get("Path"), Some("/"));
    }

    #[test]
    fn parse_aws_query_params_preserves_member_style_keys() {
        let request = request_with_query_and_body(
            None,
            Some("application/x-www-form-urlencoded"),
            b"Tags.member.1.Key=team&Tags.member.1.Value=platform&Tags.member.2.Key=env&Tags.member.2.Value=dev",
        );

        let params = AwsQueryParams::parse(&request).expect("member params should parse");

        assert_eq!(params.get("Tags.member.1.Key"), Some("team"));
        assert_eq!(params.get("Tags.member.1.Value"), Some("platform"));
        assert_eq!(params.get("Tags.member.2.Key"), Some("env"));
        assert_eq!(params.get("Tags.member.2.Value"), Some("dev"));
    }

    #[test]
    fn parse_aws_query_params_decodes_plus_and_percent_escapes() {
        let request =
            request_with_query_and_body(Some("UserName=test+user&Path=%2Fdev+ops%2F"), None, b"");

        let params = parse_aws_query_params(&request).expect("query string should parse");

        assert_eq!(params.get("UserName"), Some("test user"));
        assert_eq!(params.get("Path"), Some("/dev ops/"));
    }

    #[test]
    fn parse_aws_query_params_rejects_unsupported_body_content_type() {
        let request = request_with_query_and_body(
            None,
            Some("application/json"),
            br#"{"Action":"CreateUser"}"#,
        );

        let error = parse_aws_query_params(&request).expect_err("json body should be rejected");

        assert_eq!(
            error.to_string(),
            "Invalid AWS query request: unsupported content-type 'application/json'"
        );
    }
}
