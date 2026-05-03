use std::{net::IpAddr, str::FromStr};

use hiraeth_core::Config;
use hiraeth_iam::AuthorizationMode;
use hiraeth_runtime::{app::App, serve};
use hiraeth_store_sqlx::SqlxStore;
use hiraeth_web::WebState;
use tracing_subscriber::{EnvFilter, fmt};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    init_tracing();

    let config = config::Config::builder()
        .add_source(config::Environment::with_prefix("HIRAETH"))
        .build()?
        .try_deserialize::<Config>()?;
    tracing::info!(config = ?config, "starting Hiraeth");

    let store = SqlxStore::new(&config.database_url).await?;
    let app = std::sync::Arc::new(App::new(
        store.clone(),
        AuthorizationMode::from(config.auth_mode.clone()),
    ));
    let aws_addr = (IpAddr::from_str(&config.host)?, config.port).into();
    let aws_endpoint_url = aws_endpoint_url(&config);

    if config.web_enabled {
        let web_addr = (IpAddr::from_str(&config.web_host)?, config.web_port).into();
        tracing::info!(addr = %aws_addr, "starting AWS emulator");
        tracing::info!(addr = %web_addr, "starting web UI");

        tokio::try_join!(
            serve::serve(aws_addr, app),
            hiraeth_web::serve(
                web_addr,
                WebState::new(
                    store.iam_store.clone(),
                    store.sqs_store.clone(),
                    store.trace_store.clone()
                )
                .with_aws_endpoint_url(aws_endpoint_url)
            )
        )?;
    } else {
        tracing::info!(addr = %aws_addr, "starting AWS emulator");
        serve::serve(aws_addr, app).await?;
    }

    Ok(())
}

fn aws_endpoint_url(config: &Config) -> String {
    let host = match config.host.as_str() {
        "0.0.0.0" | "::" => "localhost",
        host => host,
    };
    let host = if host.contains(':') && !host.starts_with('[') {
        format!("[{host}]")
    } else {
        host.to_string()
    };

    format!("http://{host}:{}", config.port)
}

fn init_tracing() {
    let env_filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| {
        EnvFilter::new("hiraeth_runtime=info,hiraeth_web=info,hiraeth_iam=info,hiraeth_sns=info")
    });

    fmt().with_env_filter(env_filter).init();
}
