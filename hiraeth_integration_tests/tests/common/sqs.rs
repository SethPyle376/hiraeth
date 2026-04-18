use aws_sdk_sqs::{
    Client,
    types::{
        MessageAttributeValue, SendMessageBatchRequestEntry,
        builders::SendMessageBatchRequestEntryBuilder,
    },
};

use crate::common::{TestServer, start_test_server, unique_name};

pub struct SqsTestServer {
    pub server: TestServer,
    pub client: Client,
}

pub async fn sqs_test_server() -> anyhow::Result<SqsTestServer> {
    let server = start_test_server().await?;
    let sdk_config = server.sdk_config().await;

    Ok(SqsTestServer {
        client: Client::new(&sdk_config),
        server,
    })
}

pub fn queue_name(prefix: &str) -> String {
    unique_name(prefix)
}

pub fn string_attribute(value: &str) -> MessageAttributeValue {
    MessageAttributeValue::builder()
        .data_type("String")
        .string_value(value)
        .build()
        .expect("message attribute value should build")
}

pub fn batch_entry(id: &str, body: &str) -> SendMessageBatchRequestEntryBuilder {
    SendMessageBatchRequestEntry::builder()
        .id(id)
        .message_body(body)
}
