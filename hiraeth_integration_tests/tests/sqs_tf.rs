use aws_sdk_sqs::types::QueueAttributeName;
use serde_json::json;
use terraform_wrapper::{
    Terraform, TerraformCommand,
    config::TerraformConfig,
    prelude::{ApplyCommand, InitCommand},
};

use crate::common::sqs_test_server;

mod common;

#[tokio::test]
#[ignore] // Ignored because of TF dependency and it's slow
async fn test_sqs() -> anyhow::Result<()> {
    let server = sqs_test_server().await?;
    let config = TerraformConfig::new()
        .required_provider("aws", "hashicorp/aws", "6.41.0")
        .provider(
            "aws",
            json!({
                "region": "us-east-1",
                "access_key": "test",
                "secret_key": "test",
                "skip_credentials_validation": true,
                "skip_requesting_account_id": true,
                "skip_metadata_api_check": true,
                "endpoints": {
                    "sqs": server.server.endpoint_url()
                }
            }),
        )
        .resource(
            "aws_sqs_queue",
            "test_queue",
            json!({
                "name": "test-queue",
                "delay_seconds": 0,
                "max_message_size": 262144,
                "message_retention_seconds": 345600,
                "receive_wait_time_seconds": 0,
                "visibility_timeout_seconds": 30,
            }),
        );

    let dir = config.write_to_tempdir()?;
    let tf = Terraform::builder().working_dir(dir.path()).build()?;

    InitCommand::new().execute(&tf).await?;
    ApplyCommand::new().auto_approve().execute(&tf).await?;

    let queue_url = server
        .client
        .get_queue_url()
        .queue_name("test-queue")
        .send()
        .await?
        .queue_url
        .ok_or_else(|| anyhow::anyhow!("Queue URL not found"))?;

    let attributes = server
        .client
        .get_queue_attributes()
        .queue_url(queue_url)
        .attribute_names(QueueAttributeName::All)
        .send()
        .await?
        .attributes
        .ok_or_else(|| anyhow::anyhow!("Queue attributes not found"))?;

    assert_eq!(
        attributes.get(&QueueAttributeName::DelaySeconds).unwrap(),
        "0"
    );
    assert_eq!(
        attributes
            .get(&QueueAttributeName::MaximumMessageSize)
            .unwrap(),
        "262144"
    );
    assert_eq!(
        attributes
            .get(&QueueAttributeName::MessageRetentionPeriod)
            .unwrap(),
        "345600"
    );
    assert_eq!(
        attributes
            .get(&QueueAttributeName::ReceiveMessageWaitTimeSeconds)
            .unwrap(),
        "0"
    );
    assert_eq!(
        attributes
            .get(&QueueAttributeName::VisibilityTimeout)
            .unwrap(),
        "30"
    );

    Ok(())
}
