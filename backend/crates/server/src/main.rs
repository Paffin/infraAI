use server::config::BootConfig;
use server::{api, jobs, startup};
use std::net::SocketAddr;
use std::sync::atomic::Ordering;
use std::time::Duration;
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info,sqlx=warn,tower_http=info,hyper=warn"));
    let json = std::env::var("LOG_JSON").map(|v| matches!(v.as_str(), "1" | "true")).unwrap_or(std::env::var("APP_ENV").map(|v| v != "development").unwrap_or(true));
    if json {
        tracing_subscriber::fmt().with_env_filter(filter).json().init();
    } else {
        tracing_subscriber::fmt().with_env_filter(filter).init();
    }

    let boot = BootConfig::from_env()?;
    let bind = boot.bind;
    let grace = Duration::from_secs(boot.shutdown_grace_secs);
    tracing::info!(version = env!("CARGO_PKG_VERSION"), instance = %boot.instance_id, production = boot.production, "starting");

    let state = startup::init_state(boot).await?;
    tokio::spawn(jobs::sweeper(state.clone()));
    tokio::spawn(jobs::retention(state.clone()));

    let app = api::router(state.clone()).await;
    let listener = tokio::net::TcpListener::bind(bind).await?;
    tracing::info!(%bind, "listening");

    let st = state.clone();
    let shutdown = async move {
        wait_for_signal().await;
        // fail readiness first so the load balancer stops sending traffic, then drain
        st.shutting_down.store(true, Ordering::SeqCst);
        tracing::info!("shutdown requested: draining");
        let drain = std::env::var("DRAIN_SECONDS").ok().and_then(|v| v.parse().ok()).unwrap_or(3u64);
        tokio::time::sleep(Duration::from_secs(drain)).await;
    };
    let serve = axum::serve(listener, app.into_make_service_with_connect_info::<SocketAddr>()).with_graceful_shutdown(shutdown);
    tokio::select! {
        r = serve => { r?; }
        _ = async { wait_for_signal_then(grace).await } => { tracing::warn!("grace period elapsed, forcing shutdown"); }
    }

    // give running AI jobs a chance to finish; the sweeper on another replica handles the rest
    let deadline = std::time::Instant::now() + grace;
    while state.metrics.jobs_running.get() > 0 && std::time::Instant::now() < deadline {
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
    state.pool.close().await;
    tracing::info!("bye");
    Ok(())
}

async fn wait_for_signal() {
    #[cfg(unix)]
    {
        use tokio::signal::unix::{signal, SignalKind};
        let mut term = signal(SignalKind::terminate()).expect("SIGTERM handler");
        tokio::select! {
            _ = tokio::signal::ctrl_c() => {},
            _ = term.recv() => {},
        }
    }
    #[cfg(not(unix))]
    {
        let _ = tokio::signal::ctrl_c().await;
    }
}

async fn wait_for_signal_then(grace: Duration) {
    wait_for_signal().await;
    tokio::time::sleep(grace + Duration::from_secs(5)).await;
}
