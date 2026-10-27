//! Header-first HTTP, bounded streaming, and cancellation that wakes WASI poll.
use crate::wasm::AgentHostState;
use bytes::Bytes;
use http_body::{Body, Frame};
use http_body_util::BodyExt;
use std::{
    future::Future,
    pin::Pin,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    task::{Context, Poll, Waker},
    time::Duration,
};
use wasmtime_wasi_http::{
    WasiHttpCtx, WasiHttpView,
    bindings::http::types::ErrorCode,
    body::{HyperIncomingBody, HyperOutgoingBody},
    types::{HostFutureIncomingResponse, IncomingResponse, OutgoingRequestConfig},
};

pub const MAX_BYTES: usize = 32 * 1024 * 1024;
const TIMEOUT: Duration = Duration::from_secs(120);
const HOSTS: &[&str] = &[
    "api.openai.com",
    "api.anthropic.com",
    "openrouter.ai",
    "generativelanguage.googleapis.com",
    "api.search.brave.com",
];

/// Set the atomic AND wake all pending header/body futures. Epoch alone cannot
/// wake a synchronous WASI poll hostcall. Never set just the flag externally.
#[derive(Clone, Default)]
pub struct Abort {
    flag: Arc<AtomicBool>,
    waiters: Arc<Mutex<Vec<Waker>>>,
}
impl Abort {
    pub fn request(&self) {
        self.flag.store(true, Ordering::SeqCst);
        for waker in self.waiters.lock().unwrap().drain(..) {
            waker.wake();
        }
    }
    pub fn requested(&self) -> bool {
        self.flag.load(Ordering::SeqCst)
    }
    pub fn reset(&self) {
        self.waiters.lock().unwrap().clear();
        self.flag.store(false, Ordering::SeqCst);
    }
    fn poll_cancel(&self, cx: &mut Context<'_>) -> Poll<()> {
        let mut waiters = self.waiters.lock().unwrap();
        if self.requested() {
            return Poll::Ready(());
        }
        if !waiters.iter().any(|w| w.will_wake(cx.waker())) {
            waiters.push(cx.waker().clone());
        }
        Poll::Pending
    }
    fn cancelled(&self) -> Cancel {
        Cancel(self.clone())
    }
}
struct Cancel(Abort);
impl Future for Cancel {
    type Output = ();
    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<()> {
        self.0.poll_cancel(cx)
    }
}

#[derive(Clone, Default)]
pub struct Policy {
    // No env-var switch or production constructor can enable loopback.
    #[cfg(test)]
    pub(crate) loopback_port: Option<u16>,
}
impl Policy {
    fn check(&self, uri: &hyper::Uri, tls: bool) -> Result<(), ErrorCode> {
        #[cfg(test)]
        if !tls
            && uri.host() == Some("127.0.0.1")
            && uri.port_u16() == self.loopback_port
            && self.loopback_port.is_some()
        {
            return Ok(());
        }
        if !tls
            || uri.scheme_str() != Some("https")
            || !HOSTS.contains(&uri.host().unwrap_or(""))
            || uri.port_u16().is_some_and(|p| p != 443)
        {
            return Err(ErrorCode::HttpRequestDenied);
        }
        Ok(())
    }
}

fn inject_headers(
    request: &mut hyper::Request<HyperOutgoingBody>,
    secrets: &dyn crate::secrets::SecretStore,
) -> anyhow::Result<()> {
    let host = request.uri().host().unwrap_or("");
    let provider = match host {
        "openrouter.ai" => "openrouter",
        "api.openai.com" => "openai",
        "api.anthropic.com" => "anthropic",
        "api.search.brave.com" => "brave",
        "generativelanguage.googleapis.com" => "google",
        #[cfg(test)]
        "127.0.0.1" => "openrouter",
        _ => anyhow::bail!("unknown provider"),
    };
    let key = secrets
        .read(provider)?
        .ok_or_else(|| anyhow::anyhow!("missing provider credential"))?;
    let authority = request
        .uri()
        .authority()
        .ok_or_else(|| anyhow::anyhow!("missing authority"))?
        .as_str()
        .to_owned();
    let headers = request.headers_mut();
    headers.insert("host", authority.parse()?);
    for name in [
        "authorization",
        "x-api-key",
        "x-goog-api-key",
        "x-subscription-token",
        "proxy-authorization",
        "cookie",
        "http-referer",
        "x-title",
        "anthropic-version",
        "anthropic-beta",
        "openai-organization",
        "openai-project",
    ] {
        headers.remove(name);
    }
    match provider {
        "anthropic" => {
            headers.insert("x-api-key", key.parse()?);
            headers.insert("anthropic-version", "2023-06-01".parse()?);
        }
        "brave" => {
            headers.insert("x-subscription-token", key.parse()?);
        }
        "google" => {
            headers.insert("x-goog-api-key", key.parse()?);
        }
        _ => {
            headers.insert("authorization", format!("Bearer {key}").parse()?);
        }
    }
    if provider == "openrouter" {
        headers.insert("http-referer", "https://learning-companion.local".parse()?);
        headers.insert("x-title", "learning-companion".parse()?);
    }
    Ok(())
}

/// Reject private, local, multicast and special-purpose addresses, including
/// IPv4-mapped IPv6. DNS is resolved once, validated, then connected by IP.
fn public_ip(ip: std::net::IpAddr) -> bool {
    match ip {
        std::net::IpAddr::V4(ip) => {
            let [a, b, c, _] = ip.octets();
            !(ip.is_private()
                || ip.is_loopback()
                || ip.is_link_local()
                || ip.is_broadcast()
                || ip.is_documentation()
                || ip.is_unspecified()
                || ip.is_multicast()
                || a == 0
                || a >= 240
                || (a == 100 && (64..=127).contains(&b))
                || (a == 198 && (b == 18 || b == 19))
                || (a == 192 && ((b == 0 && c == 0) || (b == 88 && c == 99))))
        }
        std::net::IpAddr::V6(ip) => {
            if let Some(v4) = ip.to_ipv4_mapped() {
                return public_ip(v4.into());
            }
            // Only global unicast (exclude ULA/link-local/site-local/etc.).
            let segments = ip.segments();
            (segments[0] & 0xe000) == 0x2000
                && !(segments[0] == 0x2001 && (segments[1] <= 0x01ff || segments[1] == 0x0db8))
                && segments[0] != 0x2002 // 6to4 may embed a private IPv4 destination.
                && !(segments[0] == 0x3fff && (segments[1] & 0xf000) == 0) // documentation prefix
        }
    }
}

async fn pinned_request(
    mut request: hyper::Request<HyperOutgoingBody>,
    config: OutgoingRequestConfig,
) -> Result<IncomingResponse, ErrorCode> {
    use hyper_util::rt::TokioIo;
    use tokio_rustls::rustls;
    let host = request
        .uri()
        .host()
        .ok_or(ErrorCode::HttpRequestDenied)?
        .to_owned();
    let port = request
        .uri()
        .port_u16()
        .unwrap_or(if config.use_tls { 443 } else { 80 });
    let addresses: Vec<_> = tokio::net::lookup_host((host.as_str(), port))
        .await
        .map_err(|_| ErrorCode::ConnectionRefused)?
        .collect();
    let test_loopback = cfg!(test) && !config.use_tls && host == "127.0.0.1";
    if addresses.is_empty() || (!test_loopback && addresses.iter().any(|a| !public_ip(a.ip()))) {
        return Err(ErrorCode::HttpRequestDenied);
    }
    let tcp = tokio::time::timeout(
        config.connect_timeout,
        tokio::net::TcpStream::connect(addresses[0]),
    )
    .await
    .map_err(|_| ErrorCode::ConnectionTimeout)?
    .map_err(|_| ErrorCode::ConnectionRefused)?;
    let method = request.method().to_string();
    *request.uri_mut() = request
        .uri()
        .path_and_query()
        .map(|v| v.as_str())
        .unwrap_or("/")
        .parse()
        .map_err(|_| ErrorCode::HttpRequestUriInvalid)?;
    let (mut sender, worker) = if config.use_tls {
        let roots = rustls::RootCertStore {
            roots: webpki_roots::TLS_SERVER_ROOTS.into(),
        };
        let tls = rustls::ClientConfig::builder()
            .with_root_certificates(roots)
            .with_no_client_auth();
        let name = rustls::pki_types::ServerName::try_from(host.clone())
            .map_err(|_| ErrorCode::HttpRequestDenied)?;
        let stream = tokio_rustls::TlsConnector::from(Arc::new(tls))
            .connect(name, tcp)
            .await
            .map_err(|_| ErrorCode::TlsProtocolError)?;
        let (sender, conn) = hyper::client::conn::http1::handshake(TokioIo::new(stream))
            .await
            .map_err(|_| ErrorCode::ConnectionTerminated)?;
        (
            sender,
            wasmtime_wasi::runtime::spawn(async move {
                let _ = conn.await;
            }),
        )
    } else {
        let (sender, conn) = hyper::client::conn::http1::handshake(TokioIo::new(tcp))
            .await
            .map_err(|_| ErrorCode::ConnectionTerminated)?;
        (
            sender,
            wasmtime_wasi::runtime::spawn(async move {
                let _ = conn.await;
            }),
        )
    };
    let resp = tokio::time::timeout(config.first_byte_timeout, sender.send_request(request))
        .await
        .map_err(|_| ErrorCode::ConnectionReadTimeout)?
        .map_err(|_| ErrorCode::ConnectionTerminated)?;
    tracing::info!(method, host, status = resp.status().as_u16(), "http_proxy");
    Ok(IncomingResponse {
        resp: resp.map(|body| body.map_err(|_| ErrorCode::ConnectionTerminated).boxed()),
        worker: Some(worker),
        between_bytes_timeout: config.between_bytes_timeout,
    })
}

impl WasiHttpView for AgentHostState {
    fn ctx(&mut self) -> &mut WasiHttpCtx {
        &mut self.http
    }
    fn table(&mut self) -> &mut wasmtime::component::ResourceTable {
        &mut self.table
    }
    fn send_request(
        &mut self,
        request: hyper::Request<HyperOutgoingBody>,
        mut config: OutgoingRequestConfig,
    ) -> wasmtime_wasi_http::HttpResult<HostFutureIncomingResponse> {
        tracing::debug!(method = %request.method(), host = request.uri().host().unwrap_or(""), "http_proxy");

        if let Err(e) = self.policy.check(request.uri(), config.use_tls) {
            return Ok(HostFutureIncomingResponse::ready(Ok(Err(e))));
        }
        let secrets = self.secrets.clone();
        #[cfg(test)]
        let loopback_port = self.policy.loopback_port;
        config.connect_timeout = config.connect_timeout.min(TIMEOUT);
        config.first_byte_timeout = config.first_byte_timeout.min(TIMEOUT);
        config.between_bytes_timeout = config.between_bytes_timeout.min(TIMEOUT);
        let abort = self.abort.clone();
        let task = wasmtime_wasi::runtime::spawn(async move {
            let deadline = tokio::time::Instant::now() + TIMEOUT;
            let send = async move {
                // Native keychain access may block. Keep it off the agent thread
                // and inside the cancellable TTFB/total-deadline envelope.
                let request = tokio::task::spawn_blocking(move || {
                    let mut request = request;
                    inject_headers(&mut request, secrets.as_ref()).map(|()| request)
                })
                .await
                .map_err(|_| ErrorCode::HttpRequestDenied)?
                .map_err(|_| ErrorCode::HttpRequestDenied)?;
                // Only test builds can reroute the catalog's real baseUrl.
                #[cfg(test)]
                let mut request = request;
                #[cfg(test)]
                if let Some(port) = loopback_port
                    && request.uri().host() == Some("openrouter.ai")
                {
                    let path = request
                        .uri()
                        .path_and_query()
                        .map(|p| p.as_str())
                        .unwrap_or("/");
                    *request.uri_mut() = format!("http://127.0.0.1:{port}{path}").parse().unwrap();
                    config.use_tls = false;
                }
                pinned_request(request, config).await
            };
            let result = tokio::select! {
                biased;
                _ = abort.cancelled() => Err(ErrorCode::ConnectionTerminated),
                _ = tokio::time::sleep_until(deadline) => Err(ErrorCode::ConnectionReadTimeout),
                response = send => response,
            };
            Ok(result.map(|mut incoming| {
                incoming.resp = incoming.resp.map(|inner| {
                    BoundedBody {
                        inner,
                        abort,
                        total: 0,
                        done: false,
                        timeout: Box::pin(tokio::time::sleep_until(deadline)),
                    }
                    .boxed()
                });
                incoming
            }))
        });
        Ok(HostFutureIncomingResponse::pending(task))
    }
}

struct BoundedBody {
    inner: HyperIncomingBody,
    abort: Abort,
    total: usize,
    done: bool,
    timeout: Pin<Box<tokio::time::Sleep>>,
}
impl Drop for BoundedBody {
    fn drop(&mut self) {
        tracing::info!(bytes = self.total, "http_proxy");
    }
}
impl Body for BoundedBody {
    type Data = Bytes;
    type Error = ErrorCode;
    fn poll_frame(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<Frame<Bytes>, ErrorCode>>> {
        if self.done {
            return Poll::Ready(None);
        }
        let error = if self.abort.poll_cancel(cx).is_ready() {
            Some(ErrorCode::ConnectionTerminated)
        } else if self.timeout.as_mut().poll(cx).is_ready() {
            Some(ErrorCode::ConnectionReadTimeout)
        } else {
            None
        };
        if let Some(e) = error {
            self.done = true;
            return Poll::Ready(Some(Err(e)));
        }
        match Pin::new(&mut self.inner).poll_frame(cx) {
            Poll::Ready(Some(Ok(frame))) => {
                if let Some(data) = frame.data_ref() {
                    self.total += data.len();
                    if self.total > MAX_BYTES {
                        self.done = true;
                        return Poll::Ready(Some(Err(ErrorCode::HttpResponseBodySize(Some(
                            self.total as u64,
                        )))));
                    }
                }
                Poll::Ready(Some(Ok(frame)))
            }
            Poll::Ready(None) => {
                self.done = true;
                Poll::Ready(None)
            }
            other => other,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::task::{Wake, Waker};

    #[test]
    fn policy_is_allowlisted_and_loopback_is_port_scoped() {
        let production = Policy::default();
        for url in [
            "http://api.openai.com/",
            "https://evil.example/",
            "https://127.0.0.1/",
            "https://169.254.169.254/",
            "https://api.openai.com:444/",
        ] {
            let uri: hyper::Uri = url.parse().unwrap();
            assert!(
                production
                    .check(&uri, uri.scheme_str() == Some("https"))
                    .is_err()
            );
        }
        for host in HOSTS {
            assert!(
                production
                    .check(&format!("https://{host}/").parse().unwrap(), true)
                    .is_ok()
            );
        }
        for ip in [
            "10.0.0.1",
            "172.16.2.3",
            "192.168.1.2",
            "169.254.169.254",
            "127.0.0.1",
            "100.64.0.1",
            "::1",
            "fe80::1",
            "fc00::1",
            "::ffff:192.168.1.1",
        ] {
            assert!(!public_ip(ip.parse().unwrap()), "{ip}");
        }
        assert!(public_ip("8.8.8.8".parse().unwrap()));
        let test = Policy {
            loopback_port: Some(1234),
        };
        assert!(
            test.check(&"http://127.0.0.1:1234/".parse().unwrap(), false)
                .is_ok()
        );
        assert!(
            test.check(&"http://127.0.0.1:1235/".parse().unwrap(), false)
                .is_err()
        );
        assert!(
            test.check(&"http://localhost:1234/".parse().unwrap(), false)
                .is_err()
        );
    }

    #[test]
    fn credentials_overwrite_all_guest_auth_headers() {
        use crate::secrets::{MemorySecretStore, SecretStore};
        let secrets = MemorySecretStore::default();
        for (host, provider, header, expected) in [
            (
                "openrouter.ai",
                "openrouter",
                "authorization",
                "Bearer host-key",
            ),
            (
                "api.openai.com",
                "openai",
                "authorization",
                "Bearer host-key",
            ),
            ("api.anthropic.com", "anthropic", "x-api-key", "host-key"),
        ] {
            secrets.write(provider, "host-key").unwrap();
            let body = http_body_util::Empty::<Bytes>::new()
                .map_err(|never| match never {})
                .boxed();
            let mut request = hyper::Request::builder()
                .uri(format!("https://{host}/"))
                .header("authorization", "Bearer guest-dummy-not-a-secret")
                .header("authorization", "Bearer second-guest-value")
                .header("x-api-key", "guest-dummy-not-a-secret")
                .header("x-subscription-token", "guest-dummy-not-a-secret")
                .header("host", "evil.example")
                .header("x-title", "guest-title")
                .body(body)
                .unwrap();
            inject_headers(&mut request, &secrets).unwrap();
            assert_eq!(request.headers()[header], expected);
            assert_eq!(request.headers().get_all(header).iter().count(), 1);
            assert_eq!(request.headers()["host"], host);
            assert!(
                request
                    .headers()
                    .values()
                    .all(|v| !v.as_bytes().windows(5).any(|w| w == b"guest"))
            );
            if provider == "anthropic" {
                assert!(!request.headers().contains_key("authorization"));
            }
        }
    }

    struct Chunks(usize);
    impl Body for Chunks {
        type Data = Bytes;
        type Error = ErrorCode;
        fn poll_frame(
            mut self: Pin<&mut Self>,
            _: &mut Context<'_>,
        ) -> Poll<Option<Result<Frame<Bytes>, ErrorCode>>> {
            self.0 += 1;
            Poll::Ready(Some(Ok(Frame::data(Bytes::from(vec![0; 1024 * 1024])))))
        }
    }
    struct WakeCount(std::sync::atomic::AtomicUsize);
    impl Wake for WakeCount {
        fn wake(self: Arc<Self>) {
            self.0.fetch_add(1, Ordering::SeqCst);
        }
    }
    fn body(abort: Abort) -> BoundedBody {
        BoundedBody {
            inner: Chunks(0).boxed(),
            abort,
            total: 0,
            done: false,
            timeout: Box::pin(tokio::time::sleep(TIMEOUT)),
        }
    }

    #[test]
    fn running_cap_and_abort_wakeup() {
        // Use only Wasmtime's internal runtime to create timers. No runtime or
        // block_on on the agent thread (or in this test).
        wasmtime_wasi::runtime::with_ambient_tokio_runtime(|| {
            let abort = Abort::default();
            let mut body = body(abort.clone());
            let wake = Arc::new(WakeCount(std::sync::atomic::AtomicUsize::new(0)));
            let waker = Waker::from(wake.clone());
            let mut cx = Context::from_waker(&waker);
            for _ in 0..32 {
                assert!(matches!(
                    Pin::new(&mut body).poll_frame(&mut cx),
                    Poll::Ready(Some(Ok(_)))
                ));
            }
            assert!(matches!(
                Pin::new(&mut body).poll_frame(&mut cx),
                Poll::Ready(Some(Err(ErrorCode::HttpResponseBodySize(Some(34603008)))))
            ));
            assert!(matches!(
                Pin::new(&mut body).poll_frame(&mut cx),
                Poll::Ready(None)
            ));
            let mut body = self::body(abort.clone());
            assert!(matches!(
                Pin::new(&mut body).poll_frame(&mut cx),
                Poll::Ready(Some(Ok(_)))
            ));
            abort.request();
            assert!(wake.0.load(Ordering::SeqCst) > 0);
            assert!(matches!(
                Pin::new(&mut body).poll_frame(&mut cx),
                Poll::Ready(Some(Err(ErrorCode::ConnectionTerminated)))
            ));
            let mut body = self::body(Abort::default());
            body.timeout = Box::pin(tokio::time::sleep(Duration::from_millis(1)));
            // Register the timer before allowing the runtime's driver to fire it.
            let _ = body.timeout.as_mut().poll(&mut cx);
            std::thread::sleep(Duration::from_millis(20));
            assert!(matches!(
                Pin::new(&mut body).poll_frame(&mut cx),
                Poll::Ready(Some(Err(ErrorCode::ConnectionReadTimeout)))
            ));
        });
    }
}
