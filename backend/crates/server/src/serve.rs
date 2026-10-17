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

/// What `run` takes besides the listener and the router: where the limit on a client's headers comes from.
#[derive(Clone, Default)]
pub struct ServeOptions {
    header_read_timeout: HeaderReadTimeout,
}

#[derive(Clone, Default)]
enum HeaderReadTimeout {
    /// No limit.
    #[default]
    None,
    /// This long, whatever the settings say.
    Fixed(Duration),
    /// What `limits.headerReadTimeoutSeconds` says when the connection is accepted.
    Setting(Box<crate::state::AppState>),
}

impl ServeOptions {
    /// A limit that does not change: `None` = no limit.
    pub fn fixed(header_read_timeout: Option<Duration>) -> Self {
        ServeOptions { header_read_timeout: header_read_timeout.map_or(HeaderReadTimeout::None, HeaderReadTimeout::Fixed) }
    }

    /// The limit of `limits.headerReadTimeoutSeconds` (0 = none), **read for every accepted connection**, so that a change in the admin panel is in force for the
    /// next client without a restart (the settings promise that; a value read once, when the listener starts, was the only one that did not keep it). A
    /// connection already open keeps the limit it was accepted with.
    pub fn from_settings(state: &crate::state::AppState) -> Self {
        ServeOptions { header_read_timeout: HeaderReadTimeout::Setting(Box::new(state.clone())) }
    }

    /// The limit for a connection accepted now. A configuration that cannot be read (the database is away) falls back to the shipped default: a client that
    /// stalls is no more welcome then.
    pub async fn header_read_timeout(&self) -> Option<Duration> {
        let secs = match &self.header_read_timeout {
            HeaderReadTimeout::None => return None,
            HeaderReadTimeout::Fixed(d) => return Some(*d),
            HeaderReadTimeout::Setting(state) => match state.cfg().await {
                Ok(cfg) => cfg.settings.num("limits.headerReadTimeoutSeconds"),
                Err(_) => crate::settings::Settings::defaults().num("limits.headerReadTimeoutSeconds"),
            },
        };
        (secs > 0.0).then(|| Duration::from_secs_f64(secs))
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
        let (signal_tx, close_rx, options) = (signal_tx.clone(), close_rx.clone(), options.clone());
        tokio::spawn(async move {
            // asked in the task of the connection, not in the loop that accepts: reading the settings may take a query, and no client is made to wait for another's
            let header_read_timeout = options.header_read_timeout().await;
            let mut builder = Builder::new(TokioExecutor::new());
            // hyper keeps a default of its own whenever a timer is set; `header_read_timeout(None)` is how it is switched off
            builder.http1().timer(TokioTimer::new()).header_read_timeout(header_read_timeout);
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
