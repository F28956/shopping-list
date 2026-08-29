//! Counting and timing requests, without learning who they were about.

use std::time::Instant;

use axum::extract::{MatchedPath, Request};
use axum::middleware::Next;
use axum::response::Response;

/// What a request that matched no route is called.
///
/// One label rather than the path that was asked for. A 404 is the one request whose
/// path an outsider chooses, so labelling by it hands anybody who can reach the port a
/// way to write unbounded series into the scrape output — and, on a self-hosted box,
/// to fill the disk.
const UNMATCHED: &str = "unmatched";

/// Records every request that reaches a route.
///
/// Applied with `Router::layer`, which wraps each route's own service rather than the
/// router as a whole — so routing has already happened by the time this runs and
/// [`MatchedPath`] is in the extensions. That ordering is the reason the route label
/// can be a pattern at all; a layer applied outside the router would see the raw path
/// and nothing else, which is exactly what must not become a label.
pub async fn record_requests(request: Request, next: Next) -> Response {
    // Read before the request is handed on, because `next.run` consumes it. Owned for
    // the same reason.
    let route = request
        .extensions()
        .get::<MatchedPath>()
        .map(|matched| matched.as_str().to_string())
        .unwrap_or_else(|| UNMATCHED.to_string());
    let method = request.method().as_str().to_string();

    // A guard, because a handler that panics unwinds straight through this and a
    // decrement written after the await would never run. `CatchPanicLayer` turns that
    // panic into a 500 for the caller, so nothing else would look wrong — the gauge
    // would simply climb by one per panic and never come back down.
    let in_flight = InFlight::new();

    let started = Instant::now();
    let response = next.run(request).await;

    drop(in_flight);
    crate::instruments().request(
        &route,
        &method,
        response.status().as_u16(),
        started.elapsed().as_secs_f64(),
    );

    response
}

struct InFlight;

impl InFlight {
    fn new() -> Self {
        crate::instruments().request_started();
        InFlight
    }
}

impl Drop for InFlight {
    fn drop(&mut self) {
        crate::instruments().request_finished();
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use axum::Router;
    use axum::body::Body;
    use axum::extract::MatchedPath;
    use axum::http::Request;
    use axum::routing::get;
    use tower::ServiceExt;

    /// The assumption [`record_requests`] rests on, checked against axum itself rather
    /// than against its documentation.
    ///
    /// A layer added with `Router::layer` wraps each route's own service, so routing
    /// has already happened and the matched pattern is in the extensions. If that ever
    /// stopped being true the label would silently become the raw path — one time
    /// series per list id, in an endpoint a scrape can read — and nothing else in the
    /// build would notice.
    #[tokio::test]
    async fn a_route_layer_sees_the_pattern_and_not_the_path() {
        let seen: Arc<Mutex<Vec<String>>> = Arc::default();

        let recorder = {
            let seen = seen.clone();
            axum::middleware::from_fn(move |request: Request<Body>, next: axum::middleware::Next| {
                let seen = seen.clone();
                async move {
                    seen.lock().unwrap().push(
                        request
                            .extensions()
                            .get::<MatchedPath>()
                            .map(|matched| matched.as_str().to_string())
                            .unwrap_or_else(|| super::UNMATCHED.to_string()),
                    );
                    next.run(request).await
                }
            })
        };

        let app = Router::new()
            .route("/lists/{id}/items", get(|| async { "ok" }))
            .layer(recorder);

        for path in ["/lists/4108/items", "/lists/9/items", "/nothing/here"] {
            let request = Request::builder().uri(path).body(Body::empty()).unwrap();
            let _ = app.clone().oneshot(request).await.unwrap();
        }

        assert_eq!(
            *seen.lock().unwrap(),
            [
                // Two different lists, one label.
                "/lists/{id}/items",
                "/lists/{id}/items",
                // A path an outsider chose is the one path that must not become a
                // label of its own.
                super::UNMATCHED,
            ]
        );
    }
}
