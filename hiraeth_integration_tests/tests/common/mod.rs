use std::{net::SocketAddr, sync::Arc};

use anyhow::Context;
use aws_config::{BehaviorVersion, Region, SdkConfig};
use aws_credential_types::Credentials;
use hiraeth_iam::AuthorizationMode;
use hiraeth_runtime::{app::App, serve};
use hiraeth_store_sqlx::SqlxStore;
use tempfile::TempDir;
use tokio::{net::TcpListener, task::JoinHandle};
use uuid::Uuid;

mod sqs;
pub use sqs::*;

pub const DEFAULT_REGION: &str = "us-east-1";
pub const TEST_ACCESS_KEY_ID: &str = "test";
pub const TEST_SECRET_ACCESS_KEY: &str = "test";

pub struct TestServer {
    endpoint_url: String,
    _db_url: String,
    _temp_dir: TempDir,
    task: JoinHandle<()>,
}

impl TestServer {
    pub fn endpoint_url(&self) -> &str {
        &self.endpoint_url
    }

    #[allow(dead_code)]
    pub fn db_url(&self) -> &str {
        &self._db_url
    }

    pub async fn sdk_config(&self) -> SdkConfig {
        sdk_config(self.endpoint_url(), DEFAULT_REGION).await
    }
}

impl Drop for TestServer {
    fn drop(&mut self) {
        self.task.abort();
    }
}

pub async fn start_test_server() -> anyhow::Result<TestServer> {
    start_test_server_with_auth_mode(AuthorizationMode::Audit).await
}

pub async fn start_test_server_with_auth_mode(
    auth_mode: AuthorizationMode,
) -> anyhow::Result<TestServer> {
    let temp_dir = TempDir::new().context("temp dir should be created")?;
    let db_url = sqlite_url(&temp_dir);
    let store = create_store(&temp_dir).await?;
    let app = create_app(store, auth_mode);
    let listener = bind_listener().await?;
    let addr = listener.local_addr().context("listener should have addr")?;
    let endpoint_url = endpoint_url(addr);
    let task = spawn_listener(listener, app);

    Ok(TestServer {
        endpoint_url,
        _db_url: db_url,
        _temp_dir: temp_dir,
        task,
    })
}

pub async fn create_store(temp_dir: &TempDir) -> anyhow::Result<SqlxStore> {
    let db_url = sqlite_url(temp_dir);
    SqlxStore::new(&db_url)
        .await
        .context("store should initialize")
}

pub fn create_app(store: SqlxStore, auth_mode: AuthorizationMode) -> Arc<App> {
    Arc::new(App::new(store, auth_mode))
}

pub async fn bind_listener() -> anyhow::Result<TcpListener> {
    TcpListener::bind("127.0.0.1:0")
        .await
        .context("listener should bind")
}

pub fn spawn_listener(listener: TcpListener, app: Arc<App>) -> JoinHandle<()> {
    tokio::spawn(async move {
        if let Err(error) = serve::serve_listener(listener, app).await {
            eprintln!("integration test server exited: {:?}", error);
        }
    })
}

pub async fn sdk_config(endpoint_url: &str, region: &str) -> SdkConfig {
    aws_config::defaults(BehaviorVersion::latest())
        .region(Region::new(region.to_owned()))
        .endpoint_url(endpoint_url.to_owned())
        .credentials_provider(Credentials::new(
            TEST_ACCESS_KEY_ID,
            TEST_SECRET_ACCESS_KEY,
            None,
            None,
            "hiraeth-integration-tests",
        ))
        .load()
        .await
}

pub fn unique_name(prefix: &str) -> String {
    format!("{prefix}-{}", Uuid::new_v4())
}

fn sqlite_url(temp_dir: &TempDir) -> String {
    format!(
        "sqlite://{}",
        temp_dir.path().join("integration-test.sqlite").display()
    )
}

fn endpoint_url(addr: SocketAddr) -> String {
    format!("http://{addr}")
}
