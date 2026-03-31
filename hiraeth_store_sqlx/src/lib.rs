use std::str::FromStr;

mod access_key_store;

pub use access_key_store::SqliteAccessKeyStore;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StoreError {
    ConnectionError(String),
    MigrationError(String),
}

pub struct SqlxStore {
    pub access_key_store: SqliteAccessKeyStore,
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
            access_key_store: SqliteAccessKeyStore::new(&pool),
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

#[cfg(test)]
mod tests {
    use super::*;
}
