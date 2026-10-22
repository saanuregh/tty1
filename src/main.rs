mod cache;
mod client;
mod config;
mod csp;
mod providers;
mod render;
mod routes;
mod state;
mod worker;

use std::time::Duration;

use tracing::info;
use tracing_subscriber::filter::LevelFilter;

#[tokio::main]
async fn main() {
    let filter = tracing_subscriber::EnvFilter::builder()
        .with_default_directive(LevelFilter::INFO.into())
        .from_env_lossy();
    tracing_subscriber::fmt().with_env_filter(filter).init();

    let store = state::StateStore::from_config();
    let shutdown_store = store.clone();
    let data = cache::new_shared_data(store.load_feed());
    let fragments = cache::new_shared_fragments(&data);
    let html = cache::new_shared_html(&data);
    let clients = client::build_clients();

    let mut workers = Vec::new();
    let scrapers = config::scrapers_enabled();
    if scrapers {
        workers.push(tokio::spawn(worker::run_scraper(
            data.clone(),
            html.clone(),
            fragments.clone(),
            clients.direct,
            Duration::from_secs(config::SCRAPE_INTERVAL_SECS),
            config::HN_STORIES_PER_PAGE,
            store.clone(),
        )));
    } else {
        info!("background scrapers disabled");
    }

    if scrapers && config::reddit_scraper_enabled() {
        workers.push(tokio::spawn(worker::run_reddit_scraper(
            data.clone(),
            html.clone(),
            fragments.clone(),
            clients.reddit,
            store.clone(),
        )));
    } else if scrapers {
        info!("reddit scraper disabled");
    }

    workers.push(tokio::spawn(worker::run_html_refresher(
        data.clone(),
        html.clone(),
        Duration::from_secs(config::HTML_REFRESH_SECS),
    )));

    let app = routes::router(cache::AppState {
        data,
        html,
        fragments,
    });

    let addr = format!("0.0.0.0:{}", config::port());
    let listener = tokio::net::TcpListener::bind(&addr)
        .await
        .unwrap_or_else(|e| panic!("failed to bind to {addr}: {e}"));

    info!("listening on http://{addr}");
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await
        .expect("server error");
    for worker in &workers {
        worker.abort();
    }
    for worker in workers {
        let _ = worker.await;
    }
    shutdown_store.flush();
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
