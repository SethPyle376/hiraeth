use anyhow::{Context, bail};
use aws_config::{BehaviorVersion, Region, SdkConfig};
use aws_credential_types::Credentials;
use aws_sdk_sqs::{
    Client,
    types::{
        MessageAttributeValue, MessageSystemAttributeNameForSends, MessageSystemAttributeValue,
        QueueAttributeName, SendMessageBatchRequestEntry,
    },
};

const DEFAULT_ENDPOINT_URL: &str = "http://localhost:4566";
const DEFAULT_REGION: &str = "us-east-1";
const DEFAULT_ACCESS_KEY_ID: &str = "test";
const DEFAULT_SECRET_ACCESS_KEY: &str = "test";
const DEFAULT_PREFIX: &str = "hiraeth";

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SeedOptions {
    endpoint_url: String,
    region: String,
    access_key_id: String,
    secret_access_key: String,
    prefix: String,
    reset: bool,
}

impl Default for SeedOptions {
    fn default() -> Self {
        Self {
            endpoint_url: DEFAULT_ENDPOINT_URL.to_string(),
            region: DEFAULT_REGION.to_string(),
            access_key_id: DEFAULT_ACCESS_KEY_ID.to_string(),
            secret_access_key: DEFAULT_SECRET_ACCESS_KEY.to_string(),
            prefix: DEFAULT_PREFIX.to_string(),
            reset: false,
        }
    }
}

impl SeedOptions {
    pub(crate) fn parse(args: impl IntoIterator<Item = String>) -> anyhow::Result<Self> {
        let mut options = Self::default();
        let mut args = args.into_iter();

        while let Some(arg) = args.next() {
            match arg.as_str() {
                "--endpoint-url" => {
                    options.endpoint_url = normalize_endpoint_url(next_arg(&mut args, &arg)?);
                }
                "--region" => {
                    options.region = next_arg(&mut args, &arg)?;
                }
                "--access-key-id" => {
                    options.access_key_id = next_arg(&mut args, &arg)?;
                }
                "--secret-access-key" => {
                    options.secret_access_key = next_arg(&mut args, &arg)?;
                }
                "--prefix" => {
                    options.prefix = next_arg(&mut args, &arg)?;
                }
                "--reset" => {
                    options.reset = true;
                }
                "--help" | "-h" => {
                    print_seed_help();
                    std::process::exit(0);
                }
                _ => bail!("Unknown seed option: {arg}"),
            }
        }

        if options.prefix.is_empty() {
            bail!("--prefix must not be empty");
        }

        Ok(options)
    }
}

pub(crate) async fn run(options: SeedOptions) -> anyhow::Result<()> {
    let sdk_config = sdk_config(&options).await;
    let client = Client::new(&sdk_config);
    let names = QueueNames::new(&options.prefix);

    println!(
        "Seeding SQS data at {} in {} with queue prefix '{}'",
        options.endpoint_url, options.region, options.prefix
    );

    if options.reset {
        reset_seed_queues(&client, &names).await?;
    }

    let dlq_url = create_dlq(&client, &names.dead_letter).await?;
    let dlq_arn = queue_arn(&client, &dlq_url).await?;
    let orders_url = create_orders_queue(&client, &names.orders, &dlq_arn).await?;
    let work_items_url = create_work_items_queue(&client, &names.work_items).await?;
    let events_url = create_fifo_events_queue(&client, &names.events).await?;

    let seeded_orders = seed_orders_messages(&client, &orders_url).await?;
    let seeded_work_items = seed_work_items_messages(&client, &work_items_url).await?;
    let seeded_events = seed_fifo_events(&client, &events_url).await?;
    let seeded_dlq = seed_dlq_message(&client, &dlq_url).await?;

    println!("Seeded queues:");
    println!("  orders:     {orders_url}");
    println!("  work-items: {work_items_url}");
    println!("  events:     {events_url}");
    println!("  dlq:        {dlq_url}");
    println!("Seeded messages:");
    println!("  orders:     {seeded_orders}");
    println!("  work-items: {seeded_work_items}");
    println!("  events:     {seeded_events}");
    println!("  dlq:        {seeded_dlq}");

    Ok(())
}

fn print_seed_help() {
    println!("Usage: cargo run -p xtask -- seed [options]");
    println!();
    println!("Options:");
    println!("  --endpoint-url <url>       Endpoint to seed. Default: {DEFAULT_ENDPOINT_URL}");
    println!("  --region <region>          Region to sign requests for. Default: {DEFAULT_REGION}");
    println!("  --access-key-id <id>       Access key for SigV4. Default: {DEFAULT_ACCESS_KEY_ID}");
    println!(
        "  --secret-access-key <key>  Secret key for SigV4. Default: {DEFAULT_SECRET_ACCESS_KEY}"
    );
    println!("  --prefix <name>            Queue name prefix. Default: {DEFAULT_PREFIX}");
    println!("  --reset                    Delete seeded queues before recreating them");
    println!("  -h, --help                 Show this help message");
}

async fn sdk_config(options: &SeedOptions) -> SdkConfig {
    aws_config::defaults(BehaviorVersion::latest())
        .region(Region::new(options.region.clone()))
        .endpoint_url(options.endpoint_url.clone())
        .credentials_provider(Credentials::new(
            &options.access_key_id,
            &options.secret_access_key,
            None,
            None,
            "hiraeth-xtask-seed",
        ))
        .load()
        .await
}

async fn reset_seed_queues(client: &Client, names: &QueueNames) -> anyhow::Result<()> {
    for name in [
        &names.orders,
        &names.work_items,
        &names.events,
        &names.dead_letter,
    ] {
        delete_queue_if_exists(client, name).await?;
    }

    Ok(())
}

async fn delete_queue_if_exists(client: &Client, queue_name: &str) -> anyhow::Result<()> {
    let queue_url = match client.get_queue_url().queue_name(queue_name).send().await {
        Ok(response) => response
            .queue_url()
            .context("GetQueueUrl succeeded without QueueUrl")?
            .to_string(),
        Err(error)
            if error
                .as_service_error()
                .is_some_and(|error| error.is_queue_does_not_exist()) =>
        {
            return Ok(());
        }
        Err(error) => return Err(error).with_context(|| format!("get queue url for {queue_name}")),
    };

    client
        .delete_queue()
        .queue_url(&queue_url)
        .send()
        .await
        .with_context(|| format!("delete queue {queue_name}"))?;

    println!("Deleted existing queue: {queue_name}");
    Ok(())
}

async fn create_dlq(client: &Client, queue_name: &str) -> anyhow::Result<String> {
    create_queue(client, queue_name)
        .attributes(QueueAttributeName::MessageRetentionPeriod, "1209600")
        .attributes(QueueAttributeName::ReceiveMessageWaitTimeSeconds, "0")
        .tags("service", "sqs")
        .tags("role", "dead-letter")
        .tags("seeded-by", "hiraeth-xtask")
        .send()
        .await
        .with_context(|| format!("create queue {queue_name}"))?
        .queue_url()
        .context("CreateQueue succeeded without QueueUrl")
        .map(ToOwned::to_owned)
}

async fn create_orders_queue(
    client: &Client,
    queue_name: &str,
    dlq_arn: &str,
) -> anyhow::Result<String> {
    let redrive_policy = format!(r#"{{"deadLetterTargetArn":"{dlq_arn}","maxReceiveCount":"3"}}"#);

    create_queue(client, queue_name)
        .attributes(QueueAttributeName::VisibilityTimeout, "45")
        .attributes(QueueAttributeName::ReceiveMessageWaitTimeSeconds, "2")
        .attributes(QueueAttributeName::MessageRetentionPeriod, "1209600")
        .attributes(QueueAttributeName::RedrivePolicy, redrive_policy)
        .attributes(QueueAttributeName::SqsManagedSseEnabled, "false")
        .tags("service", "sqs")
        .tags("domain", "orders")
        .tags("environment", "local")
        .tags("seeded-by", "hiraeth-xtask")
        .send()
        .await
        .with_context(|| format!("create queue {queue_name}"))?
        .queue_url()
        .context("CreateQueue succeeded without QueueUrl")
        .map(ToOwned::to_owned)
}

async fn create_work_items_queue(client: &Client, queue_name: &str) -> anyhow::Result<String> {
    create_queue(client, queue_name)
        .attributes(QueueAttributeName::VisibilityTimeout, "20")
        .attributes(QueueAttributeName::ReceiveMessageWaitTimeSeconds, "1")
        .attributes(QueueAttributeName::MaximumMessageSize, "262144")
        .tags("service", "sqs")
        .tags("domain", "workers")
        .tags("environment", "local")
        .tags("seeded-by", "hiraeth-xtask")
        .send()
        .await
        .with_context(|| format!("create queue {queue_name}"))?
        .queue_url()
        .context("CreateQueue succeeded without QueueUrl")
        .map(ToOwned::to_owned)
}

async fn create_fifo_events_queue(client: &Client, queue_name: &str) -> anyhow::Result<String> {
    create_queue(client, queue_name)
        .attributes(QueueAttributeName::FifoQueue, "true")
        .attributes(QueueAttributeName::ContentBasedDeduplication, "true")
        .attributes(QueueAttributeName::DeduplicationScope, "messageGroup")
        .attributes(QueueAttributeName::FifoThroughputLimit, "perMessageGroupId")
        .tags("service", "sqs")
        .tags("domain", "events")
        .tags("environment", "local")
        .tags("seeded-by", "hiraeth-xtask")
        .send()
        .await
        .with_context(|| format!("create queue {queue_name}"))?
        .queue_url()
        .context("CreateQueue succeeded without QueueUrl")
        .map(ToOwned::to_owned)
}

fn create_queue<'a>(
    client: &'a Client,
    queue_name: &'a str,
) -> aws_sdk_sqs::operation::create_queue::builders::CreateQueueFluentBuilder {
    client.create_queue().queue_name(queue_name)
}

async fn queue_arn(client: &Client, queue_url: &str) -> anyhow::Result<String> {
    let response = client
        .get_queue_attributes()
        .queue_url(queue_url)
        .attribute_names(QueueAttributeName::QueueArn)
        .send()
        .await
        .context("get queue arn")?;

    response
        .attributes()
        .and_then(|attributes| attributes.get(&QueueAttributeName::QueueArn))
        .context("GetQueueAttributes succeeded without QueueArn")
        .map(ToOwned::to_owned)
}

async fn seed_orders_messages(client: &Client, queue_url: &str) -> anyhow::Result<usize> {
    client
        .send_message()
        .queue_url(queue_url)
        .message_body(
            r#"{"order_id":"ord-1001","customer_id":"cust-42","total_cents":3299,"status":"created"}"#,
        )
        .message_attributes("event_type", string_attribute("order.created")?)
        .message_attributes("tenant", string_attribute("acme")?)
        .message_attributes("priority", number_attribute("1")?)
        .message_system_attributes(
            MessageSystemAttributeNameForSends::AwsTraceHeader,
            trace_header("Root=1-65f1a37a-9b7d6f95d3cf4f76a1f0b2a3")?,
        )
        .send()
        .await
        .context("send created order message")?;

    client
        .send_message()
        .queue_url(queue_url)
        .message_body(
            r#"{"order_id":"ord-1002","customer_id":"cust-17","total_cents":10999,"status":"paid"}"#,
        )
        .message_attributes("event_type", string_attribute("order.paid")?)
        .message_attributes("tenant", string_attribute("globex")?)
        .message_attributes("priority", number_attribute("2")?)
        .send()
        .await
        .context("send paid order message")?;

    client
        .send_message()
        .queue_url(queue_url)
        .message_body(
            r#"{"order_id":"ord-1003","customer_id":"cust-42","total_cents":1899,"status":"fulfillment_pending"}"#,
        )
        .message_attributes("event_type", string_attribute("order.fulfillment_pending")?)
        .message_attributes("tenant", string_attribute("acme")?)
        .delay_seconds(30)
        .send()
        .await
        .context("send delayed order message")?;

    Ok(3)
}

async fn seed_work_items_messages(client: &Client, queue_url: &str) -> anyhow::Result<usize> {
    let response = client
        .send_message_batch()
        .queue_url(queue_url)
        .entries(batch_entry(
            "thumbnail",
            r#"{"job_id":"job-2001","kind":"thumbnail"}"#,
        )?)
        .entries(batch_entry(
            "email",
            r#"{"job_id":"job-2002","kind":"email"}"#,
        )?)
        .entries(batch_entry(
            "report",
            r#"{"job_id":"job-2003","kind":"report"}"#,
        )?)
        .send()
        .await
        .context("send work item batch")?;

    client
        .receive_message()
        .queue_url(queue_url)
        .message_attribute_names("All")
        .max_number_of_messages(1)
        .visibility_timeout(120)
        .send()
        .await
        .context("mark one work item in-flight")?;

    Ok(response.successful().len())
}

async fn seed_fifo_events(client: &Client, queue_url: &str) -> anyhow::Result<usize> {
    for (dedup_id, body) in [
        (
            "seed-fifo-1",
            r#"{"event_id":"evt-3001","aggregate_id":"customer-42","type":"CustomerUpdated"}"#,
        ),
        (
            "seed-fifo-2",
            r#"{"event_id":"evt-3002","aggregate_id":"customer-42","type":"CustomerNotified"}"#,
        ),
    ] {
        client
            .send_message()
            .queue_url(queue_url)
            .message_body(body)
            .message_group_id("customer-42")
            .message_deduplication_id(dedup_id)
            .message_attributes("event_type", string_attribute("customer.event")?)
            .send()
            .await
            .with_context(|| format!("send fifo event {dedup_id}"))?;
    }

    Ok(2)
}

async fn seed_dlq_message(client: &Client, queue_url: &str) -> anyhow::Result<usize> {
    client
        .send_message()
        .queue_url(queue_url)
        .message_body(
            r#"{"order_id":"ord-0999","reason":"example poison message for local debugging"}"#,
        )
        .message_attributes("event_type", string_attribute("order.failed")?)
        .message_attributes("failure_count", number_attribute("3")?)
        .send()
        .await
        .context("send dlq message")?;

    Ok(1)
}

fn batch_entry(id: &str, body: &str) -> anyhow::Result<SendMessageBatchRequestEntry> {
    SendMessageBatchRequestEntry::builder()
        .id(id)
        .message_body(body)
        .message_attributes("job_kind", string_attribute(id)?)
        .message_attributes("attempt", number_attribute("1")?)
        .build()
        .context("build batch message entry")
}

fn string_attribute(value: &str) -> anyhow::Result<MessageAttributeValue> {
    MessageAttributeValue::builder()
        .data_type("String")
        .string_value(value)
        .build()
        .context("build string message attribute")
}

fn number_attribute(value: &str) -> anyhow::Result<MessageAttributeValue> {
    MessageAttributeValue::builder()
        .data_type("Number")
        .string_value(value)
        .build()
        .context("build number message attribute")
}

fn trace_header(value: &str) -> anyhow::Result<MessageSystemAttributeValue> {
    MessageSystemAttributeValue::builder()
        .data_type("String")
        .string_value(value)
        .build()
        .context("build trace header system attribute")
}

fn next_arg(args: &mut impl Iterator<Item = String>, option_name: &str) -> anyhow::Result<String> {
    args.next()
        .with_context(|| format!("{option_name} requires a value"))
}

fn normalize_endpoint_url(endpoint_url: String) -> String {
    if endpoint_url.starts_with("http://") || endpoint_url.starts_with("https://") {
        endpoint_url
    } else {
        format!("http://{endpoint_url}")
    }
}

struct QueueNames {
    orders: String,
    work_items: String,
    events: String,
    dead_letter: String,
}

impl QueueNames {
    fn new(prefix: &str) -> Self {
        Self {
            orders: format!("{prefix}-orders"),
            work_items: format!("{prefix}-work-items"),
            events: format!("{prefix}-events.fifo"),
            dead_letter: format!("{prefix}-dlq"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{DEFAULT_ENDPOINT_URL, DEFAULT_PREFIX, DEFAULT_REGION, SeedOptions};

    #[test]
    fn seed_options_default_to_local_hiraeth_endpoint() {
        let options = SeedOptions::parse([]).expect("default options should parse");

        assert_eq!(options.endpoint_url, DEFAULT_ENDPOINT_URL);
        assert_eq!(options.region, DEFAULT_REGION);
        assert_eq!(options.access_key_id, "test");
        assert_eq!(options.secret_access_key, "test");
        assert_eq!(options.prefix, DEFAULT_PREFIX);
        assert!(!options.reset);
    }

    #[test]
    fn seed_options_parse_overrides() {
        let options = SeedOptions::parse([
            "--endpoint-url".to_string(),
            "localhost:9999".to_string(),
            "--region".to_string(),
            "us-west-2".to_string(),
            "--access-key-id".to_string(),
            "key".to_string(),
            "--secret-access-key".to_string(),
            "secret".to_string(),
            "--prefix".to_string(),
            "demo".to_string(),
            "--reset".to_string(),
        ])
        .expect("options should parse");

        assert_eq!(options.endpoint_url, "http://localhost:9999");
        assert_eq!(options.region, "us-west-2");
        assert_eq!(options.access_key_id, "key");
        assert_eq!(options.secret_access_key, "secret");
        assert_eq!(options.prefix, "demo");
        assert!(options.reset);
    }

    #[test]
    fn seed_options_reject_unknown_option() {
        let result = SeedOptions::parse(["--bogus".to_string()]);

        assert!(result.is_err());
    }
}
