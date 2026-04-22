use hiraeth_http::IncomingRequest;
use hiraeth_store::iam::{AccessKey, AccessKeyStore};
use hmac::{Hmac, KeyInit, Mac};
use sha2::{Digest, Sha256};

use crate::AuthError;

/**
 * Implements AWS Signature Version 4 authentication for incoming HTTP requests.
**/
pub(crate) async fn authenticate_request<S: AccessKeyStore>(
    request: &IncomingRequest,
    store: &S,
) -> Result<(SigV4AuthParameters, AccessKey), AuthError> {
    let sig_v4_params = extract_sigv4_params(request)?;

    let access_key = store
        .get_secret_key(&sig_v4_params.access_key)
        .await
        .map_err(AuthError::KeyStoreError)?
        .ok_or(AuthError::SecretKeyNotFound)?;

    let provided_signature = sig_v4_params.signature.clone();
    let calculated_signature = hash_request(request, &access_key).await?;

    if provided_signature == calculated_signature {
        Ok((sig_v4_params, access_key))
    } else {
        Err(AuthError::InvalidSignature)
    }
}

async fn hash_request(
    request: &IncomingRequest,
    access_key: &AccessKey,
) -> Result<String, AuthError> {
    let sigv4_params = extract_sigv4_params(request)?;
    let canonical_request = create_canonical_request(request, &sigv4_params.signed_headers)?;
    let hashed_canonical_request = hex::encode(Sha256::digest(canonical_request.as_bytes()));
    let request_timestamp = request
        .headers
        .get("x-amz-date")
        .ok_or(AuthError::MissingSignedHeader("x-amz-date".to_string()))?;
    if !request_timestamp.starts_with(&sigv4_params.date) {
        return Err(AuthError::InvalidAuthorizationHeader(
            "Credential date does not match x-amz-date".to_string(),
        ));
    }

    let string_to_sign = format!(
        "{}\n{}\n{}/{}/{}/aws4_request\n{}",
        sigv4_params.algorithm,
        request_timestamp,
        sigv4_params.date,
        sigv4_params.region,
        sigv4_params.service,
        hashed_canonical_request
    );

    let signing_key = derive_signing_key(
        access_key.secret_key.as_str(),
        &sigv4_params.date,
        &sigv4_params.region,
        &sigv4_params.service,
    );

    let mut mac =
        Hmac::<Sha256>::new_from_slice(&signing_key).expect("HMAC can take key of any size");
    mac.update(string_to_sign.as_bytes());

    let signature = hex::encode(mac.finalize().into_bytes());
    Ok(signature)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SigV4AuthParameters {
    pub algorithm: String,
    pub access_key: String,
    pub date: String,
    pub region: String,
    pub service: String,
    pub signed_headers: Vec<String>,
    pub signature: String,
}

fn extract_sigv4_params(request: &IncomingRequest) -> Result<SigV4AuthParameters, AuthError> {
    let auth_header = request
        .headers
        .get("authorization")
        .ok_or(AuthError::MissingAuthorizationHeader)?;

    let split = auth_header.split_once(' ');
    let (algorithm, key_value_pairs) = split.ok_or(AuthError::InvalidAuthorizationHeader(
        "Authorization header format invalid".to_string(),
    ))?;

    if algorithm != "AWS4-HMAC-SHA256" {
        return Err(AuthError::InvalidAuthorizationHeader(format!(
            "Unsupported algorithm: {}",
            algorithm
        )));
    }

    let mut credential = None;
    let mut signed_headers = None;
    let mut signature = None;

    for pair in key_value_pairs.trim().split(',').map(str::trim) {
        let (key, value) = pair
            .split_once('=')
            .ok_or(AuthError::InvalidAuthorizationHeader(format!(
                "Authorization parameter '{}' is malformed",
                pair
            )))?;

        match key {
            "Credential" => credential = Some(value),
            "SignedHeaders" => signed_headers = Some(value),
            "Signature" => signature = Some(value),
            _ => {}
        }
    }

    let credential = credential.ok_or(AuthError::InvalidAuthorizationHeader(
        "Credential missing".to_string(),
    ))?;
    let credential_parts = credential.split('/').collect::<Vec<_>>();
    if credential_parts.len() != 5 {
        return Err(AuthError::InvalidAuthorizationHeader(
            "Credential scope format invalid".to_string(),
        ));
    }
    if credential_parts[4] != "aws4_request" {
        return Err(AuthError::InvalidAuthorizationHeader(
            "Credential scope terminator invalid".to_string(),
        ));
    }

    let signed_headers = signed_headers
        .ok_or(AuthError::InvalidAuthorizationHeader(
            "Signed headers missing".to_string(),
        ))?
        .split(';')
        .filter(|header| !header.is_empty())
        .map(|s| s.to_string())
        .collect::<Vec<_>>();
    if signed_headers.is_empty() {
        return Err(AuthError::InvalidAuthorizationHeader(
            "Signed headers value missing".to_string(),
        ));
    }

    let signature = signature
        .ok_or(AuthError::InvalidAuthorizationHeader(
            "Signature missing".to_string(),
        ))?
        .to_string();
    if signature.is_empty() {
        return Err(AuthError::InvalidAuthorizationHeader(
            "Signature value missing".to_string(),
        ));
    }

    Ok(SigV4AuthParameters {
        algorithm: algorithm.to_string(),
        access_key: credential_parts[0].to_string(),
        date: credential_parts[1].to_string(),
        region: credential_parts[2].to_string(),
        service: credential_parts[3].to_string(),
        signed_headers,
        signature,
    })
}

fn create_canonical_request(
    request: &IncomingRequest,
    signed_headers: &[String],
) -> Result<String, AuthError> {
    let mut canonical_request = String::new();
    canonical_request.push_str(&request.method);
    canonical_request.push('\n');
    canonical_request.push_str(&canonical_uri(&request.path));
    canonical_request.push('\n');

    canonical_request.push_str(&canonical_query_string(request));
    canonical_request.push('\n');

    let signed_headers = canonicalize_signed_headers(signed_headers);
    let signed_headers_str = extract_signed_headers(request, &signed_headers)?;
    canonical_request.push_str(&signed_headers_str);
    canonical_request.push('\n');
    canonical_request.push('\n');
    canonical_request.push_str(&signed_headers.join(";"));
    canonical_request.push('\n');

    let payload_hash = hex::encode(Sha256::digest(&request.body));
    canonical_request.push_str(&payload_hash);

    Ok(canonical_request)
}

fn extract_query_params(request: &IncomingRequest) -> Vec<(String, String)> {
    if let Some(query) = &request.query {
        let mut params = query
            .split('&')
            .map(|pair| {
                let (key, value) = pair
                    .split_once('=')
                    .map_or((pair, ""), |(key, value)| (key, value));
                (key.to_string(), value.to_string())
            })
            .collect::<Vec<_>>();
        params.sort_unstable_by(|(left_key, left_value), (right_key, right_value)| {
            let left_key = aws_uri_encode(left_key, true);
            let right_key = aws_uri_encode(right_key, true);

            left_key.cmp(&right_key).then_with(|| {
                aws_uri_encode(left_value, true).cmp(&aws_uri_encode(right_value, true))
            })
        });
        params
    } else {
        Vec::new()
    }
}

fn extract_signed_headers(
    request: &IncomingRequest,
    signed_headers: &[String],
) -> Result<String, AuthError> {
    let mut headers = Vec::new();
    for header in signed_headers {
        let value = request
            .headers
            .get(header)
            .ok_or(AuthError::MissingSignedHeader(header.clone()))?;
        headers.push(format!("{}:{}", header, normalize_header_value(value)));
    }
    Ok(headers.join("\n"))
}

fn canonical_uri(path: &str) -> String {
    if path.is_empty() {
        "/".to_string()
    } else {
        aws_uri_encode(path, false)
    }
}

fn canonical_query_string(request: &IncomingRequest) -> String {
    extract_query_params(request)
        .into_iter()
        .map(|(key, value)| {
            format!(
                "{}={}",
                aws_uri_encode(&key, true),
                aws_uri_encode(&value, true)
            )
        })
        .collect::<Vec<_>>()
        .join("&")
}

fn canonicalize_signed_headers(signed_headers: &[String]) -> Vec<String> {
    let mut canonical = signed_headers
        .iter()
        .map(|header| header.to_ascii_lowercase())
        .collect::<Vec<_>>();
    canonical.sort_unstable();
    canonical.dedup();
    canonical
}

fn normalize_header_value(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn aws_uri_encode(value: &str, encode_slash: bool) -> String {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";

    let mut encoded = String::with_capacity(value.len());
    for byte in value.as_bytes() {
        match *byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                encoded.push(*byte as char);
            }
            b'/' if !encode_slash => encoded.push('/'),
            _ => {
                encoded.push('%');
                encoded.push(HEX[(byte >> 4) as usize] as char);
                encoded.push(HEX[(byte & 0x0F) as usize] as char);
            }
        }
    }

    encoded
}

fn hmac_bytes(key: &[u8], data: &[u8]) -> Vec<u8> {
    let mut mac = Hmac::<Sha256>::new_from_slice(key).expect("HMAC can take key of any size");
    mac.update(data);
    mac.finalize().into_bytes().to_vec()
}

fn derive_signing_key(secret_key: &str, date: &str, region: &str, service: &str) -> Vec<u8> {
    let k_date = hmac_bytes(format!("AWS4{}", secret_key).as_bytes(), date.as_bytes());
    let k_region = hmac_bytes(&k_date, region.as_bytes());
    let k_service = hmac_bytes(&k_region, service.as_bytes());
    hmac_bytes(&k_service, b"aws4_request")
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use hiraeth_http::IncomingRequest;
    use hiraeth_store::iam::{AccessKey, InMemoryAccessKeyStore};

    use crate::AuthError;

    use super::{
        authenticate_request, create_canonical_request, derive_signing_key, extract_sigv4_params,
        hash_request,
    };

    fn access_key_record() -> AccessKey {
        AccessKey {
            key_id: "AKIAIOSFODNN7EXAMPLE".to_string(),
            principal_id: 1,
            secret_key: "wJalrXUtnFEMI/K7MDENG/bPxRfiCYEXAMPLEKEY".to_string(),
            created_at: chrono::Utc::now().naive_utc(),
        }
    }

    fn access_key_store() -> InMemoryAccessKeyStore {
        InMemoryAccessKeyStore::new([access_key_record()])
    }

    fn signed_request(signature: &str) -> IncomingRequest {
        let mut headers = HashMap::new();
        headers.insert("content-type".to_string(), "application/json".to_string());
        headers.insert(
            "host".to_string(),
            "sqs.us-east-1.amazonaws.com".to_string(),
        );
        headers.insert("x-amz-date".to_string(), "20260330T120000Z".to_string());
        headers.insert(
            "authorization".to_string(),
            format!(
                "AWS4-HMAC-SHA256 Credential=AKIAIOSFODNN7EXAMPLE/20260330/us-east-1/sqs/aws4_request, SignedHeaders=content-type;host;x-amz-date, Signature={signature}"
            ),
        );

        IncomingRequest {
            host: "sqs.us-east-1.amazonaws.com".to_string(),
            method: "POST".to_string(),
            path: "/hello".to_string(),
            query: Some("b=two&a=one".to_string()),
            headers,
            body: "hello world".to_string().into_bytes(),
        }
    }

    #[test]
    fn extracts_sigv4_parameters_from_authorization_header() {
        let mut headers = HashMap::new();
        headers.insert(
            "authorization".to_string(),
            "AWS4-HMAC-SHA256 Credential=AKIAIOSFODNN7EXAMPLE/20260328/us-east-1/sqs/aws4_request, SignedHeaders=content-type;host;x-amz-date, Signature=deadbeef1234".to_string(),
        );

        let request = IncomingRequest {
            host: "sqs.us-east-1.amazonaws.com".to_string(),
            method: "POST".to_string(),
            path: "/".to_string(),
            query: None,
            headers,
            body: Vec::new(),
        };

        let params = extract_sigv4_params(&request).expect("authorization header should parse");

        assert_eq!(params.algorithm, "AWS4-HMAC-SHA256");
        assert_eq!(params.access_key, "AKIAIOSFODNN7EXAMPLE");
        assert_eq!(params.date, "20260328");
        assert_eq!(params.region, "us-east-1");
        assert_eq!(params.service, "sqs");
        assert_eq!(
            params.signed_headers,
            vec![
                "content-type".to_string(),
                "host".to_string(),
                "x-amz-date".to_string()
            ]
        );
        assert_eq!(params.signature, "deadbeef1234");
    }

    #[test]
    fn create_canonical_request_includes_all_sigv4_sections() {
        let request = signed_request("placeholder");

        let signed_headers = vec![
            "content-type".to_string(),
            "host".to_string(),
            "x-amz-date".to_string(),
        ];

        let canonical_request =
            create_canonical_request(&request, &signed_headers).expect("canonical request");

        assert_eq!(
            canonical_request,
            concat!(
                "POST\n",
                "/hello\n",
                "a=one&b=two\n",
                "content-type:application/json\n",
                "host:sqs.us-east-1.amazonaws.com\n",
                "x-amz-date:20260330T120000Z\n",
                "\n",
                "content-type;host;x-amz-date\n",
                "b94d27b9934d3e08a52e52d7da7dabfac484efe37a5380ee9088f7ace2efcde9"
            )
        );
    }

    #[test]
    fn create_canonical_request_sorts_query_params_after_encoding() {
        let mut headers = HashMap::new();
        headers.insert(
            "host".to_string(),
            "sqs.us-east-1.amazonaws.com".to_string(),
        );
        headers.insert("x-amz-date".to_string(), "20260330T120000Z".to_string());

        let request = IncomingRequest {
            host: "sqs.us-east-1.amazonaws.com".to_string(),
            method: "GET".to_string(),
            path: "/".to_string(),
            query: Some("aZ=2&a{=1".to_string()),
            headers,
            body: Vec::new(),
        };

        let canonical_request =
            create_canonical_request(&request, &["x-amz-date".to_string(), "host".to_string()])
                .expect("canonical request");

        assert!(canonical_request.starts_with(
            "GET\n/\na%7B=1&aZ=2\nhost:sqs.us-east-1.amazonaws.com\nx-amz-date:20260330T120000Z\n\nhost;x-amz-date\n"
        ));
    }

    #[test]
    fn create_canonical_request_normalizes_header_values_and_sorts_signed_headers() {
        let mut headers = HashMap::new();
        headers.insert(
            "host".to_string(),
            "  sqs.us-east-1.amazonaws.com  ".to_string(),
        );
        headers.insert("x-amz-date".to_string(), "20260330T120000Z".to_string());
        headers.insert("x-amz-meta-test".to_string(), "a   b\tc".to_string());

        let request = IncomingRequest {
            host: "sqs.us-east-1.amazonaws.com".to_string(),
            method: "GET".to_string(),
            path: "/".to_string(),
            query: None,
            headers,
            body: Vec::new(),
        };

        let canonical_request = create_canonical_request(
            &request,
            &[
                "x-amz-meta-test".to_string(),
                "host".to_string(),
                "x-amz-date".to_string(),
            ],
        )
        .expect("canonical request");

        assert!(canonical_request.contains(
            "host:sqs.us-east-1.amazonaws.com\nx-amz-date:20260330T120000Z\nx-amz-meta-test:a b c\n\nhost;x-amz-date;x-amz-meta-test\n"
        ));
    }

    #[test]
    fn derive_signing_key_matches_aws_example_vector() {
        let signing_key = derive_signing_key(
            "wJalrXUtnFEMI/K7MDENG+bPxRfiCYEXAMPLEKEY",
            "20120215",
            "us-east-1",
            "iam",
        );

        assert_eq!(
            hex::encode(signing_key),
            "f4780e2d9f65fa895f9c67b32ce1baf0b0d8a43505a000a1a9e090d414db404d"
        );
    }

    #[tokio::test]
    async fn hash_request_matches_expected_signature() {
        let request = signed_request("placeholder");
        let access_key = access_key_record();

        let signature = hash_request(&request, &access_key)
            .await
            .expect("request should hash");

        assert_eq!(
            signature,
            "ffff699a5016d0166b23b26521afd5147ba0d923ca7ec1153d95db81e1cbce6c"
        );
    }

    #[tokio::test]
    async fn authenticate_request_accepts_matching_signature() {
        let request =
            signed_request("ffff699a5016d0166b23b26521afd5147ba0d923ca7ec1153d95db81e1cbce6c");
        let store = access_key_store();

        let result = authenticate_request(&request, &store).await;

        assert!(matches!(
            result,
            Ok((ref params, ref access_key))
                if params.access_key == "AKIAIOSFODNN7EXAMPLE"
                    && params.region == "us-east-1"
                    && params.service == "sqs"
                    && access_key.principal_id == 1
        ));
    }

    #[tokio::test]
    async fn authenticate_request_rejects_invalid_signature() {
        let request = signed_request("not-the-right-signature");
        let store = access_key_store();

        let result = authenticate_request(&request, &store).await;

        assert_eq!(result, Err(AuthError::InvalidSignature));
    }
}
