use std::{convert::Infallible, net::SocketAddr, sync::Arc};

use http_body_util::Full;
use hyper::{
    Request,
    body::{Bytes, Incoming},
    server::conn::http1,
    service::service_fn,
};
use hyper_util::rt::TokioIo;
use tokio::net::TcpListener;

use crate::app::App;

pub async fn serve(addr: SocketAddr, app: Arc<App>) -> anyhow::Result<()> {
    let listener = TcpListener::bind(addr).await?;

    loop {
        let (stream, _) = listener.accept().await?;
        let app = Arc::clone(&app);

        let service = service_fn(move |request| handle_request(Arc::clone(&app), request));
        tokio::spawn(async move {
            let io = TokioIo::new(stream);
            if let Err(e) = http1::Builder::new().serve_connection(io, service).await {
                eprintln!("Error serving connection: {:?}", e);
            }
        });
    }
}

async fn handle_request(
    app: Arc<App>,
    request: Request<Incoming>,
) -> Result<hyper::Response<Full<Bytes>>, Infallible> {
    let incoming_request = hiraeth_http::IncomingRequest::from_hyper(request)
        .await
        .inspect_err(|e| eprintln!("Failed to convert request: {:?}", e))
        .unwrap();

    let response = app.handle_request(incoming_request).await;

    match response {
        Ok(response) => {
            let mut builder = hyper::Response::builder().status(response.status_code);
            Ok(builder.body(Full::from(response.body)).unwrap())
        }
        Err(e) => {
            eprintln!("Error handling request: {:?}", e);
            let mut builder = hyper::Response::builder().status(e.status_code());
            Ok(builder.body(Full::from(e.message())).unwrap())
        }
    }
}
