use std::{net::IpAddr, str::FromStr};

use crate::app::App;

mod app;
mod serve;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let config = config::Config::builder()
        .add_source(config::Environment::with_prefix("HIRAETH"))
        .build()?
        .try_deserialize::<hiraeth_core::Config>()?;
    println!("Starting Hiraeth with config: {:?}", config);

    let app = App::new(&config).await?;
    serve::serve(
        (IpAddr::from_str(&config.host)?, config.port).into(),
        std::sync::Arc::new(app),
    )
    .await
}
