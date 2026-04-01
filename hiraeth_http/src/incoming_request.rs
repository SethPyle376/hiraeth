use std::collections::HashMap;

use http_body_util::BodyExt;
use hyper::body::Incoming;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IncomingRequest {
    pub method: String,
    pub path: String,
    pub query: Option<String>,
    pub headers: HashMap<String, String>,
    pub body: Vec<u8>,
}

impl IncomingRequest {
    fn from_parts(parts: http::request::Parts, body: Vec<u8>) -> Self {
        IncomingRequest {
            method: parts.method.to_string(),
            path: parts.uri.path().to_string(),
            query: parts.uri.query().map(|q| q.to_string()),
            headers: parts
                .headers
                .iter()
                .map(|(k, v)| {
                    (
                        k.to_string().to_ascii_lowercase(),
                        v.to_str().unwrap_or("").to_string(),
                    )
                })
                .collect(),
            body,
        }
    }

    pub async fn from_hyper(req: hyper::Request<Incoming>) -> Result<Self, hyper::Error> {
        let (parts, body) = req.into_parts();
        let bytes = body.collect().await?.to_bytes();

        Ok(Self::from_parts(parts, bytes.to_vec()))
    }
}

#[cfg(test)]
mod tests {
    use http::Method;

    use super::*;

    #[test]
    fn from_parts_captures_request_parts_and_body() {
        let request = http::Request::builder()
            .method(Method::POST)
            .uri("/hello?name=world")
            .header("host", "example.test")
            .header("content-type", "text/plain")
            .header("x-custom", "abc")
            .body(())
            .unwrap();
        let (parts, _) = request.into_parts();
        let request = IncomingRequest::from_parts(parts, b"hello world".to_vec());

        assert_eq!(request.method, "POST");
        assert_eq!(request.path, "/hello");
        assert_eq!(request.query, Some("name=world".to_string()));
        assert_eq!(request.headers.get("host").unwrap(), "example.test");
        assert_eq!(request.headers.get("content-type").unwrap(), "text/plain");
        assert_eq!(request.headers.get("x-custom").unwrap(), "abc");
        assert_eq!(request.body, b"hello world");
    }
}
