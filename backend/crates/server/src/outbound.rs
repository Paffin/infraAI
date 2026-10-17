//! What the server reads from other hosts (the release tables, the model provider): the answer is read with a ceiling on its size.
//!
//! A `Content-Length` check is not a ceiling: a chunked answer has none, and `Response::bytes`/`text`/`json` buffer the whole body before anything
//! looks at it, so a source that never stops makes the server hold the body in memory until the request's own timeout. [`read_capped`] counts
//! the bytes as they arrive and stops at the ceiling (`outbound.maxAnswerKb`), whatever the headers promised.
//!
//! reqwest is built here without `gzip` or `brotli`, so an answer is read as it is on the wire: the bytes counted are the bytes held. Keep it that
//! way, or count after decoding (a small body can inflate a thousandfold).

/// How long a connection to another host may take to be made. One value for both clients, here and nowhere else: it only decides how fast an address that
/// does not answer is given up (the whole request has its own bound from a setting - `llm.timeoutSeconds`, `llm.catalogTimeoutSeconds`,
/// `versions.syncTimeoutSeconds` - which reqwest cannot be given once for a client that is built when the server starts).
pub const CONNECT_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);

/// The builder both clients start from: who the server says it is, and the time to connect.
pub fn client_builder() -> reqwest::ClientBuilder {
    reqwest::Client::builder().user_agent(concat!("infra-architect/", env!("CARGO_PKG_VERSION"))).connect_timeout(CONNECT_TIMEOUT)
}

/// The client of the model provider (`AppState::http`) keeps reqwest's redirect policy: the provider's own redirects are its business, and reqwest drops the
/// `Authorization` header when a redirect changes the host.
pub fn llm_client() -> reqwest::Result<reqwest::Client> {
    client_builder().build()
}

/// The client for fetching from a source that is held to an allowlist (`versions::fetch`): it never follows a redirect by itself, because the one who
/// follows decides each hop against the allowlist.
pub fn sync_client() -> reqwest::Result<reqwest::Client> {
    client_builder().redirect(reqwest::redirect::Policy::none()).build()
}

/// The message of an answer that went past its ceiling. `LlmError::Other` and the sync outcome carry it as it is.
pub const TOO_LARGE: &str = "the answer is too large";

/// The ceiling for one answer, from the setting `outbound.maxAnswerKb`.
pub fn answer_cap(settings: &crate::settings::Settings) -> usize {
    (settings.num("outbound.maxAnswerKb").max(1.0) as usize).saturating_mul(1024)
}

/// The body of `res`, at most `cap` bytes of it. `Err(TOO_LARGE)` as soon as the promised length or the bytes received exceed the cap (the rest of the
/// answer is not read: dropping the response closes the connection); any other error is the transport's own words, without the address.
pub async fn read_capped(mut res: reqwest::Response, cap: usize) -> Result<Vec<u8>, String> {
    if res.content_length().is_some_and(|n| n > cap as u64) {
        return Err(TOO_LARGE.into());
    }
    let mut body: Vec<u8> = Vec::with_capacity(res.content_length().map(|n| n as usize).unwrap_or(8 * 1024).min(cap));
    while let Some(chunk) = res.chunk().await.map_err(|e| e.without_url().to_string())? {
        if body.len().saturating_add(chunk.len()) > cap {
            return Err(TOO_LARGE.into());
        }
        body.extend_from_slice(&chunk);
    }
    Ok(body)
}

/// [`read_capped`] and parse the body as JSON.
pub async fn read_json_capped(res: reqwest::Response, cap: usize) -> Result<serde_json::Value, String> {
    let bytes = read_capped(res, cap).await?;
    serde_json::from_slice(&bytes).map_err(|e| format!("not JSON: {e}"))
}

/// Sockets that misbehave on purpose, for the tests of this module and of `versions`. Doubles of services are not used in these tests (docs/DEVELOPMENT.md),
/// but no real service can be asked to redirect somewhere it should not, or to answer for ever: a listener on 127.0.0.1 that speaks just enough HTTP/1.1 by hand
/// is the only way to see what the client does then. It counts the requests it gets, which is what the tests assert on.
#[cfg(test)]
pub(crate) mod hostile {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::{TcpListener, TcpStream};

    pub enum Reply {
        /// The bytes of a whole answer, written as they are.
        Fixed(Vec<u8>),
        /// `200` with `Transfer-Encoding: chunked` and no end and no length, until the client hangs up.
        Endless,
        /// `200` with `Transfer-Encoding: chunked`: these bodies as chunks, then the end.
        Chunked(Vec<Vec<u8>>),
    }

    pub struct Listener {
        pub port: u16,
        hits: Arc<AtomicUsize>,
    }

    impl Listener {
        /// The requests this listener has been asked for.
        pub fn hits(&self) -> usize {
            self.hits.load(Ordering::SeqCst)
        }
        pub fn url(&self, path: &str) -> String {
            format!("http://127.0.0.1:{}{path}", self.port)
        }
    }

    pub fn answer(status: &str, headers: &[(&str, &str)], body: &str) -> Reply {
        let mut out = format!("HTTP/1.1 {status}\r\nConnection: close\r\n");
        if !headers.iter().any(|(k, _)| k.eq_ignore_ascii_case("content-length")) {
            out.push_str(&format!("Content-Length: {}\r\n", body.len()));
        }
        for (k, v) in headers {
            out.push_str(&format!("{k}: {v}\r\n"));
        }
        out.push_str("\r\n");
        out.push_str(body);
        Reply::Fixed(out.into_bytes())
    }

    pub fn json(body: &str) -> Reply {
        answer("200 OK", &[("Content-Type", "application/json")], body)
    }

    pub fn redirect(status: u16, location: &str) -> Reply {
        answer(&format!("{status} Redirect"), &[("Location", location)], "")
    }

    async fn request_path(sock: &mut TcpStream) -> Option<String> {
        let mut head = Vec::new();
        let mut buf = [0u8; 1024];
        while !head.windows(4).any(|w| w == b"\r\n\r\n") {
            let n = sock.read(&mut buf).await.ok().filter(|n| *n > 0)?;
            head.extend_from_slice(&buf[..n]);
            if head.len() > 16 * 1024 {
                return None;
            }
        }
        String::from_utf8_lossy(&head).lines().next()?.split_whitespace().nth(1).map(String::from)
    }

    /// Listen on a free port of 127.0.0.1; every request is answered with what `reply` makes of its path.
    pub async fn listen(reply: impl Fn(&str) -> Reply + Send + Sync + 'static) -> Listener {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("a free port");
        let port = listener.local_addr().unwrap().port();
        let hits = Arc::new(AtomicUsize::new(0));
        let (counted, reply) = (hits.clone(), Arc::new(reply));
        tokio::spawn(async move {
            while let Ok((mut sock, _)) = listener.accept().await {
                let (counted, reply) = (counted.clone(), reply.clone());
                tokio::spawn(async move {
                    let Some(path) = request_path(&mut sock).await else { return };
                    counted.fetch_add(1, Ordering::SeqCst);
                    let chunked = "HTTP/1.1 200 OK\r\nConnection: close\r\nContent-Type: application/json\r\nTransfer-Encoding: chunked\r\n\r\n";
                    match reply(&path) {
                        Reply::Fixed(bytes) => {
                            let _ = sock.write_all(&bytes).await;
                        }
                        Reply::Chunked(parts) => {
                            let mut out = chunked.as_bytes().to_vec();
                            for p in parts {
                                out.extend_from_slice(format!("{:x}\r\n", p.len()).as_bytes());
                                out.extend_from_slice(&p);
                                out.extend_from_slice(b"\r\n");
                            }
                            out.extend_from_slice(b"0\r\n\r\n");
                            let _ = sock.write_all(&out).await;
                        }
                        Reply::Endless => {
                            if sock.write_all(chunked.as_bytes()).await.is_ok() {
                                let piece = [b'x'; 512];
                                // ends when the client closes the connection (the write fails)
                                while sock.write_all(b"200\r\n").await.is_ok() && sock.write_all(&piece).await.is_ok() && sock.write_all(b"\r\n").await.is_ok() {}
                            }
                        }
                    }
                    let _ = sock.shutdown().await;
                });
            }
        });
        Listener { port, hits }
    }
}

#[cfg(test)]
mod tests {
    use super::hostile::*;
    use super::*;
    use std::time::{Duration, Instant};

    async fn get(url: String) -> reqwest::Response {
        sync_client().unwrap().get(url).timeout(Duration::from_secs(10)).send().await.unwrap()
    }

    #[tokio::test]
    async fn an_answer_within_the_cap_is_read_whole_with_or_without_a_length() {
        let sized = listen(|_| json(r#"{"a":1}"#)).await;
        assert_eq!(read_capped(get(sized.url("/x")).await, 1024).await.unwrap(), br#"{"a":1}"#);
        let chunked = listen(|_| Reply::Chunked(vec![br#"{"a":"#.to_vec(), br#"1}"#.to_vec()])).await;
        assert_eq!(read_json_capped(get(chunked.url("/x")).await, 1024).await.unwrap(), serde_json::json!({"a": 1}), "chunks are put together");
    }

    #[tokio::test]
    async fn an_answer_that_announces_too_much_is_refused_before_its_body_is_read() {
        let big = listen(|_| answer("200 OK", &[("Content-Length", "5000000")], "short")).await;
        let started = Instant::now();
        assert_eq!(read_capped(get(big.url("/x")).await, 4096).await.unwrap_err(), TOO_LARGE);
        assert!(started.elapsed() < Duration::from_secs(2), "the promised length was enough: the body (which never comes) was not waited for");
    }

    #[tokio::test]
    async fn an_oversized_chunked_answer_is_cut_off_at_the_cap() {
        // no Content-Length, no end: only the bytes counted as they come can stop it
        let endless = listen(|_| Reply::Endless).await;
        let started = Instant::now();
        let res = get(endless.url("/x")).await;
        assert!(res.content_length().is_none(), "the answer has no length: that is the point");
        assert_eq!(read_capped(res, 1024).await.unwrap_err(), TOO_LARGE);
        assert!(started.elapsed() < Duration::from_secs(2), "cut off at once, not at the request's timeout: {:?}", started.elapsed());
        // the same body, finite and within the cap, is read: the cap is the only thing that stopped the first
        let finite = listen(|_| Reply::Chunked(vec![vec![b'x'; 512]; 2])).await;
        assert_eq!(read_capped(get(finite.url("/x")).await, 1024).await.unwrap().len(), 1024, "exactly the cap is allowed");
        assert_eq!(read_capped(get(finite.url("/x")).await, 1023).await.unwrap_err(), TOO_LARGE, "one byte more is not");
    }

    #[test]
    fn the_cap_comes_from_the_setting_in_kilobytes() {
        let d = crate::settings::Settings::defaults();
        assert_eq!(answer_cap(&d), 4096 * 1024);
        let s = d.with("outbound.maxAnswerKb", serde_json::json!(64)).unwrap();
        assert_eq!(answer_cap(&s), 64 * 1024);
        assert!(crate::settings::Settings::defaults().with("outbound.maxAnswerKb", serde_json::json!(1)).is_err(), "below the floor: nothing could be read");
    }
}
