use std::{net::IpAddr, str::FromStr};

use hiraeth_runtime::{app::App, serve};
use hiraeth_store_sqlx::SqlxStore;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let config = config::Config::builder()
        .add_source(config::Environment::with_prefix("HIRAETH"))
        .build()?
        .try_deserialize::<hiraeth_core::Config>()?;
    println!("Starting Hiraeth with config: {:?}", config);

    let store = SqlxStore::new(&config.database_url).await?;
    let app = std::sync::Arc::new(App::new(store.clone()));
    let aws_addr = (IpAddr::from_str(&config.host)?, config.port).into();

    if config.web_enabled {
        let web_addr = (IpAddr::from_str(&config.web_host)?, config.web_port).into();
        println!("Starting AWS emulator on {}", aws_addr);
        println!("Starting web UI on {}", web_addr);

        tokio::try_join!(
            serve::serve(aws_addr, app),
            hiraeth_web::serve(
                web_addr,
                hiraeth_web::WebState::new(store.sqs_store.clone())
            )
        )?;
    } else {
        println!("Starting AWS emulator on {}", aws_addr);
        serve::serve(aws_addr, app).await?;
    }

    Ok(())
}
