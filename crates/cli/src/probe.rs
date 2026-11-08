//! Health probes for the HTTP transport.
//!
//! Serves `GET /healthz` (liveness) and `GET /readyz` (readiness) for
//! Kubernetes probes, load balancers, and gateways. The probes are
//! unauthenticated, so readiness bounds how often and how long it reaches
//! the database, and its bodies never carry a failure cause.

use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use axum::extract::State;
use axum::http::StatusCode;
use axum::routing::get;
use tokio::sync::Mutex;
use tokio::time::Instant;
use tokio_util::sync::CancellationToken;

/// Boxed future resolving to whether the database is reachable.
pub(crate) type PingFuture = Pin<Box<dyn Future<Output = bool> + Send>>;

/// Builds the router serving `/healthz` and `/readyz`.
///
/// `ping` checks the database; it is called at most once a second and
/// cut off after two seconds, whatever the database timeouts.
/// Readiness fails once `shutdown` is cancelled, so orchestrators stop
/// routing before the pools drain.
pub(crate) fn router<F>(ping: F, shutdown: CancellationToken) -> axum::Router
where
    F: Fn() -> PingFuture + Send + Sync + 'static,
{
    axum::Router::new()
        .route("/healthz", get(healthz))
        .route("/readyz", get(readyz))
        .with_state(Readiness {
            ping: Arc::new(ping),
            shutdown,
            last: Arc::new(Mutex::new(None)),
        })
}

/// Liveness probe: answers `200 ok` while the process serves requests.
async fn healthz() -> &'static str {
    "ok"
}

/// Readiness probe: `200 ok` when the database answers, else `503`.
async fn readyz(State(readiness): State<Readiness>) -> (StatusCode, &'static str) {
    if readiness.check().await {
        (StatusCode::OK, "ok")
    } else {
        (StatusCode::SERVICE_UNAVAILABLE, "unavailable")
    }
}

/// Shared, rate-limited readiness check behind [`readyz`].
///
/// Concurrent requests share one in-flight ping and reuse its result
/// for [`Readiness::CACHE_TTL`], so a flood costs at most one database
/// query per window.
#[derive(Clone)]
struct Readiness {
    ping: Arc<dyn Fn() -> PingFuture + Send + Sync>,
    shutdown: CancellationToken,
    last: Arc<Mutex<Option<(Instant, bool)>>>,
}

impl Readiness {
    /// How long a ping result answers later probes.
    const CACHE_TTL: Duration = Duration::from_secs(1);

    /// Upper bound on a single ping, independent of database timeouts.
    const DEADLINE: Duration = Duration::from_secs(2);

    async fn check(&self) -> bool {
        if self.shutdown.is_cancelled() {
            return false;
        }
        // Holding the lock across the ping makes concurrent probes wait
        // for it and then read its cached result.
        let mut last = self.last.lock().await;
        if let Some((at, ready)) = *last
            && at.elapsed() < Self::CACHE_TTL
        {
            return ready;
        }
        let ready = tokio::time::timeout(Self::DEADLINE, (self.ping)())
            .await
            .unwrap_or(false);
        *last = Some((Instant::now(), ready));
        ready
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::sync::atomic::{AtomicUsize, Ordering};

    use axum::body::{Body, to_bytes};
    use axum::http::Request;
    use tower::ServiceExt;

    /// Returns a probe router whose ping sleeps `delay`, answers `ready`, and counts calls.
    fn scripted(ready: bool, delay: Duration) -> (axum::Router, Arc<AtomicUsize>, CancellationToken) {
        let calls = Arc::new(AtomicUsize::new(0));
        let shutdown = CancellationToken::new();
        let counter = calls.clone();
        let ping = move || -> PingFuture {
            counter.fetch_add(1, Ordering::SeqCst);
            Box::pin(async move {
                tokio::time::sleep(delay).await;
                ready
            })
        };
        (router(ping, shutdown.clone()), calls, shutdown)
    }

    async fn get(router: &axum::Router, uri: &str) -> (StatusCode, String) {
        let request = Request::builder().uri(uri).body(Body::empty()).expect("build request");
        let response = router.clone().oneshot(request).await.expect("oneshot");
        let status = response.status();
        let bytes = to_bytes(response.into_body(), 1024).await.expect("body bytes");
        (status, String::from_utf8_lossy(&bytes).into_owned())
    }

    #[tokio::test(start_paused = true)]
    async fn healthz_answers_ok() {
        let (router, calls, _shutdown) = scripted(false, Duration::ZERO);
        assert_eq!(get(&router, "/healthz").await, (StatusCode::OK, "ok".into()));
        assert_eq!(calls.load(Ordering::SeqCst), 0, "liveness never pings");
    }

    #[tokio::test(start_paused = true)]
    async fn readyz_mirrors_ping() {
        let (ready, _, _shutdown) = scripted(true, Duration::ZERO);
        assert_eq!(get(&ready, "/readyz").await, (StatusCode::OK, "ok".into()));
        let (down, _, _shutdown) = scripted(false, Duration::ZERO);
        assert_eq!(
            get(&down, "/readyz").await,
            (StatusCode::SERVICE_UNAVAILABLE, "unavailable".into())
        );
    }

    #[tokio::test(start_paused = true)]
    async fn readyz_fails_when_ping_exceeds_deadline() {
        let (router, _, _shutdown) = scripted(true, Duration::from_secs(5));
        let start = Instant::now();
        assert_eq!(get(&router, "/readyz").await.0, StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(start.elapsed(), Readiness::DEADLINE);
    }

    #[tokio::test(start_paused = true)]
    async fn concurrent_probes_share_one_ping() {
        let (router, calls, _shutdown) = scripted(true, Duration::from_millis(100));
        let mut probes = tokio::task::JoinSet::new();
        for _ in 0..10 {
            let router = router.clone();
            probes.spawn(async move { get(&router, "/readyz").await.0 });
        }
        while let Some(status) = probes.join_next().await {
            assert_eq!(status.expect("probe task"), StatusCode::OK);
        }
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test(start_paused = true)]
    async fn result_is_reused_until_ttl_expires() {
        let (router, calls, _shutdown) = scripted(true, Duration::ZERO);
        get(&router, "/readyz").await;
        get(&router, "/readyz").await;
        assert_eq!(calls.load(Ordering::SeqCst), 1, "fresh result answers without a ping");
        tokio::time::advance(Readiness::CACHE_TTL).await;
        get(&router, "/readyz").await;
        assert_eq!(calls.load(Ordering::SeqCst), 2, "expired result triggers a new ping");
    }

    #[tokio::test(start_paused = true)]
    async fn readyz_fails_once_shutdown_starts() {
        let (router, calls, shutdown) = scripted(true, Duration::ZERO);
        shutdown.cancel();
        assert_eq!(get(&router, "/readyz").await.0, StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(calls.load(Ordering::SeqCst), 0, "shutdown answers without a ping");
    }
}
