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
    types::{HostFutureIncomingResponse, OutgoingRequestConfig, default_send_request_handler},
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

#[derive(Default)]
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
            || !HOSTS.contains(&uri.host().unwrap_or(""))
            || uri.port_u16().is_some_and(|p| p != 443)
        {
            return Err(ErrorCode::HttpRequestDenied);
        }
        // Spike is deliberately fail-closed for external requests: PR5 must
        // supply keychain overwrite AND DNS pinning/private-address rejection.
        // Never send guest Authorization (including the dummy) to a provider.
        Err(ErrorCode::HttpRequestDenied)
    }
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
        mut request: hyper::Request<HyperOutgoingBody>,
        mut config: OutgoingRequestConfig,
    ) -> wasmtime_wasi_http::HttpResult<HostFutureIncomingResponse> {
        #[cfg(test)]
        eprintln!(
            "HTTP method={} host={} tls={}",
            request.method(),
            request.uri().host().unwrap_or(""),
            config.use_tls
        );
        if let Err(e) = self.policy.check(request.uri(), config.use_tls) {
            return Ok(HostFutureIncomingResponse::ready(Ok(Err(e))));
        }
        request.headers_mut().remove("authorization");
        request.headers_mut().remove("x-api-key");
        request.headers_mut().remove("x-subscription-token");
        config.connect_timeout = config.connect_timeout.min(TIMEOUT);
        config.first_byte_timeout = config.first_byte_timeout.min(TIMEOUT);
        config.between_bytes_timeout = config.between_bytes_timeout.min(TIMEOUT);
        let abort = self.abort.clone();
        let task = wasmtime_wasi::runtime::spawn(async move {
            let deadline = tokio::time::Instant::now() + TIMEOUT;
            let result = tokio::select! {
                biased;
                _ = abort.cancelled() => Err(ErrorCode::ConnectionTerminated),
                _ = tokio::time::sleep_until(deadline) => Err(ErrorCode::ConnectionReadTimeout),
                response = default_send_request_handler(request, config) => response,
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
    fn policy_is_fail_closed_and_loopback_is_port_scoped() {
        let production = Policy::default();
        for url in [
            "http://api.openai.com/",
            "https://evil.example/",
            "https://127.0.0.1/",
            "https://169.254.169.254/",
            "https://api.openai.com:444/",
            "https://api.openai.com/",
        ] {
            let uri: hyper::Uri = url.parse().unwrap();
            assert!(
                production
                    .check(&uri, uri.scheme_str() == Some("https"))
                    .is_err()
            );
        }
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
