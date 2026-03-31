pub async fn get_store_pool(url: &str) -> Result<sqlx::SqlitePool, sqlx::Error> {
    let pool = sqlx::SqlitePool::connect(url).await?;
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
