use std::{
    net::{IpAddr, Ipv4Addr},
    str::FromStr,
};

use crate::app::App;

mod app;
mod serve;

#[tokio::main]
async fn main() {
    let config = config::Config::builder()
        .add_source(config::Environment::with_prefix("HIRAETH"))
        .build()
        .expect("Failed to build configuration")
        .try_deserialize::<hiraeth_core::Config>()
        .expect("Failed to load configuration");
    println!("Starting Hiraeth with config: {:?}", config);

    let app = App::new(&config).await;
    serve::serve(
        (
            IpAddr::V4(
                Ipv4Addr::from_str(&config.host).expect("Failed to parse host as IP address"),
            ),
            config.port,
        )
            .into(),
        std::sync::Arc::new(app),
    )
    .await
    .unwrap();
}
