mod app;
mod serve;

#[tokio::main]
async fn main() {
    let app = app::App::new("sqlite://db.sqlite").await;
    serve::serve(([127, 0, 0, 1], 8080).into(), std::sync::Arc::new(app))
        .await
        .unwrap();
}
