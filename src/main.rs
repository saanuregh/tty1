mod cache;
mod client;
mod config;
mod providers;
mod render;
mod routes;
mod worker;

use std::time::Duration;

use tracing::info;

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    let data = cache::new_shared_data();
    let html = cache::new_shared_html();
    let client = client::build_client();

    tokio::spawn(worker::run_scraper(
        data.clone(),
        html.clone(),
        client,
        Duration::from_secs(config::SCRAPE_INTERVAL_SECS),
        config::HN_STORIES_PER_PAGE,
    ));

    tokio::spawn(worker::run_html_refresher(
        data.clone(),
        html.clone(),
        Duration::from_secs(config::HTML_REFRESH_SECS),
    ));

    let app = routes::router(cache::AppState { data, html });

    let addr = format!("0.0.0.0:{}", config::port());
    let listener = tokio::net::TcpListener::bind(&addr)
        .await
        .unwrap_or_else(|e| panic!("failed to bind to {addr}: {e}"));

    info!("listening on http://{addr}");
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await
        .expect("server error");
}

async fn shutdown_signal() {
    let ctrl_c = tokio::signal::ctrl_c();
    #[cfg(unix)]
    {
        let mut sigterm = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("failed to register SIGTERM handler");
        tokio::select! {
            _ = ctrl_c => info!("received SIGINT, shutting down"),
            _ = sigterm.recv() => info!("received SIGTERM, shutting down"),
        }
    }
    #[cfg(not(unix))]
    {
        ctrl_c.await.expect("failed to listen for ctrl-c");
        info!("received SIGINT, shutting down");
    }
}
