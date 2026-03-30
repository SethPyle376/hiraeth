#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServiceResponse {
    pub status_code: u16,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

impl From<ServiceResponse> for http::Response<Vec<u8>> {
    fn from(value: ServiceResponse) -> http::Response<Vec<u8>> {
        let mut response = http::Response::builder().status(value.status_code);
        for (key, header_value) in value.headers {
            response = response.header(key, header_value);
        }
        response.body(value.body).unwrap()
    }
}

#[cfg(test)]
mod tests {
    use super::ServiceResponse;

    #[test]
    fn converts_into_http_response_with_status_headers_and_body() {
        let response = ServiceResponse {
            status_code: 202,
            headers: vec![
                ("content-type".to_string(), "application/json".to_string()),
                ("x-amzn-requestid".to_string(), "req-123".to_string()),
            ],
            body: br#"{"ok":true}"#.to_vec(),
        };

        let http_response: http::Response<Vec<u8>> = response.into();

        assert_eq!(http_response.status(), http::StatusCode::ACCEPTED);
        assert_eq!(
            http_response
                .headers()
                .get("content-type")
                .unwrap()
                .to_str()
                .unwrap(),
            "application/json"
        );
        assert_eq!(
            http_response
                .headers()
                .get("x-amzn-requestid")
                .unwrap()
                .to_str()
                .unwrap(),
            "req-123"
        );
        assert_eq!(http_response.body(), br#"{"ok":true}"#);
    }
}
