use std::str::FromStr;

mod iam;
mod sqs;
mod trace;

pub use iam::{SqliteAccessKeyStore, SqliteIamStore, SqlitePrincipalStore};
pub use sqs::SqliteSqsStore;
pub use trace::SqliteTraceStore;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StoreError {
    ConnectionError(String),
    MigrationError(String),
}

impl std::fmt::Display for StoreError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            StoreError::ConnectionError(msg) => write!(f, "connection error: {}", msg),
            StoreError::MigrationError(msg) => write!(f, "migration error: {}", msg),
        }
    }
}

impl std::error::Error for StoreError {}

#[derive(Clone)]
pub struct SqlxStore {
    pub sqs_store: SqliteSqsStore,
    pub iam_store: SqliteIamStore,
    pub trace_store: SqliteTraceStore,
}

impl SqlxStore {
    pub async fn new(db_url: &str) -> Result<Self, StoreError> {
        let pool = get_store_pool(db_url)
            .await
            .map_err(|err| StoreError::ConnectionError(err.to_string()))?;
        run_migrations(&pool)
            .await
            .map_err(|err| StoreError::MigrationError(err.to_string()))?;
        Ok(Self {
            sqs_store: SqliteSqsStore::new(&pool),
            iam_store: SqliteIamStore::new(&pool),
            trace_store: SqliteTraceStore::new(&pool),
        })
    }
}

pub async fn get_store_pool(url: &str) -> Result<sqlx::SqlitePool, sqlx::Error> {
    let options = sqlx::sqlite::SqliteConnectOptions::from_str(url)?
        .create_if_missing(true)
        .foreign_keys(true)
        .journal_mode(sqlx::sqlite::SqliteJournalMode::Wal)
        .busy_timeout(std::time::Duration::from_secs(5));
    let pool = sqlx::SqlitePool::connect_with(options).await?;
    Ok(pool)
}

pub async fn run_migrations(pool: &sqlx::SqlitePool) -> Result<(), sqlx::migrate::MigrateError> {
    static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!("./migrations");
    MIGRATOR.run(pool).await
}
