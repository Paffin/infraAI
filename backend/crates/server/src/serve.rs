//! Serving a router on a listener: `axum::serve` with the one thing it cannot be asked for, a limit on how long a client may take to send the headers of a request.
//!
//! `axum::serve` builds its connection without a timer, so hyper's `header_read_timeout` is not in force and a client that sends `GET / HTTP/1.1` and one header
//! line and then nothing keeps its connection (and the task and the buffers behind it) for ever - measured: a half-open request was still open after 95 s. A
//! deadline around the handlers (`limits.requestTimeoutSeconds`) starts only after the headers have been read and cannot help. Behind a reverse proxy the proxy
//! does this (nginx `client_header_timeout`); on a port that is open straight to the network - the quick start - nothing did. So the connections are served
//! here, with hyper's builder and a timer, in the way `axum::serve` serves them: the peer address goes into the request for `ConnectInfo`, HTTP/1.1 is spoken
//! (axum is built here without its `http2` feature, and so is hyper), and on the shutdown signal the connections finish what they are doing and close.

use axum::body::Body;
use axum::extract::ConnectInfo;
use axum::Router;
use futures::FutureExt;
use hyper::body::Incoming;
use hyper_util::rt::{TokioExecutor, TokioIo, TokioTimer};
use hyper_util::server::conn::auto::Builder;
use hyper_util::service::TowerToHyperService;
use std::future::Future;
use std::io;
use std::net::SocketAddr;
use std::pin::pin;
use std::time::Duration;
use tokio::net::TcpListener;
use tokio::sync::watch;
use tower::ServiceExt;

/// What `run` takes besides the listener and the router.
#[derive(Debug, Clone, Copy, Default)]
pub struct ServeOptions {
    /// How long a client has to send the headers of a request (and, on a connection kept open, to start the next one). `None`: no limit.
    pub header_read_timeout: Option<Duration>,
}

impl ServeOptions {
    /// The options from the settings (`limits.headerReadTimeoutSeconds`; 0 = none). Read once, when the listener starts.
    pub async fn from_settings(state: &crate::state::AppState) -> Self {
        let secs = match state.cfg().await {
            Ok(cfg) => cfg.settings.num("limits.headerReadTimeoutSeconds"),
            Err(_) => crate::settings::Settings::defaults().num("limits.headerReadTimeoutSeconds"),
        };
        ServeOptions { header_read_timeout: (secs > 0.0).then(|| Duration::from_secs_f64(secs)) }
    }
}

/// An error of `accept` that is the fault of one connection and not of the listener: nothing to wait for.
fn is_connection_error(e: &io::Error) -> bool {
    matches!(e.kind(), io::ErrorKind::ConnectionRefused | io::ErrorKind::ConnectionAborted | io::ErrorKind::ConnectionReset)
}

/// Serve `app` on `listener` until `shutdown` completes, then let the requests in flight finish and return. The request carries the address of its peer
/// (`ConnectInfo<SocketAddr>`), as with `into_make_service_with_connect_info`.
pub async fn run(listener: TcpListener, app: Router, options: ServeOptions, shutdown: impl Future<Output = ()> + Send + 'static) -> io::Result<()> {
    // `signal_rx` lives as long as the shutdown is awaited: when it is dropped, `signal_tx.closed()` completes, for the accept loop and for every connection
    let (signal_tx, signal_rx) = watch::channel(());
    tokio::spawn(async move {
        shutdown.await;
        drop(signal_rx);
    });
    // every connection holds a `close_rx`; `close_tx.closed()` completes when the last of them has finished
    let (close_tx, close_rx) = watch::channel(());

    loop {
        let (stream, peer) = tokio::select! {
            accepted = listener.accept() => match accepted {
                Ok(connection) => connection,
                Err(e) => {
                    if !is_connection_error(&e) {
                        // out of file descriptors, most likely: waiting is what lets the ones in use finish
                        tracing::error!(error = %e, "accept failed");
                        tokio::time::sleep(Duration::from_secs(1)).await;
                    }
                    continue;
                }
            },
            _ = signal_tx.closed() => break,
        };
        let service = TowerToHyperService::new(app.clone().map_request(move |mut req: axum::extract::Request<Incoming>| {
            req.extensions_mut().insert(ConnectInfo::<SocketAddr>(peer));
            req.map(Body::new)
        }));
        let (signal_tx, close_rx) = (signal_tx.clone(), close_rx.clone());
        tokio::spawn(async move {
            let mut builder = Builder::new(TokioExecutor::new());
            // hyper keeps a default of its own whenever a timer is set; `header_read_timeout(None)` is how it is switched off
            builder.http1().timer(TokioTimer::new()).header_read_timeout(options.header_read_timeout);
            let mut connection = pin!(builder.serve_connection_with_upgrades(TokioIo::new(stream), service));
            let mut stopping = pin!(signal_tx.closed().fuse());
            loop {
                tokio::select! {
                    result = connection.as_mut() => {
                        if let Err(e) = result {
                            tracing::debug!(error = %e, "connection ended with an error");
                        }
                        break;
                    }
                    _ = &mut stopping => connection.as_mut().graceful_shutdown(),
                }
            }
            drop(close_rx);
        });
    }

    drop(close_rx);
    drop(listener);
    close_tx.closed().await;
    Ok(())
}
