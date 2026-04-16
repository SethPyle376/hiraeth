mod seed;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let task = std::env::args().nth(1).unwrap_or("help".to_string());

    match task.as_str() {
        "prepare-db" => {
            std::fs::create_dir_all(".local")?;
            let db_url = "sqlite://.local/db.sqlite";
            let pool = hiraeth_store_sqlx::get_store_pool(db_url).await?;
            hiraeth_store_sqlx::run_migrations(&pool).await?;
            println!("Database prepared successfully.");
            std::process::exit(0);
        }
        "seed" => {
            let options = seed::SeedOptions::parse(std::env::args().skip(2))?;
            seed::run(options).await?;
        }
        "help" => {
            println!("Usage: cargo run -p xtask -- <task>");
            println!("Tasks:");
            println!("  prepare-db - Prepare the database by running migrations");
            println!("  seed       - Seed SQS data through an AWS-compatible endpoint");
            println!("  help       - Show this help message");
            std::process::exit(1);
        }
        _ => {
            println!("Unknown task: {}", task);
            println!("Use 'help' to see available tasks.");
            std::process::exit(1);
        }
    };

    Ok(())
}
