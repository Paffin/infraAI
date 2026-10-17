//! A gate on the requests that cost this replica's processors for seconds: the verdict of a design, a failure simulation, the plan of improvements, a
//! placement, an export, the writes that evaluate what they write.
//!
//! What it protects is CPU, and CPU is per replica: a design of 459 nodes takes 8.6 s to evaluate alone on a debug build and 30 s each when eight are asked
//! at once (measured), and nothing said no. So the gate counts what is *running*, not what was *asked per minute*: a minute of 170 evaluations is
//! one editor with a debounce of 350 ms and is no abuse, while three that have not finished are three processors. A person may have
//! `limits.heavyConcurrentPerUser` of them in flight and the replica `limits.heavyConcurrentPerCore` per processor; past either the answer is
//! `429 rate_limited` with `Retry-After`.
//!
//! Two things make the count mean something:
//!
//! * The slot is released when the **work** ends, not when the request does. The editor aborts the previous evaluation on every edit; a request that is
//!   aborted drops its future, and a blocking task is not stopped by that: it goes on burning the processor. So the [`HeavyGuard`] is moved into the closure
//!   that runs on the blocking pool ([`crate::api_support::blocking`]), and the slot goes back when the last holder - the handler or the closure - lets go.
//! * The state is in memory, per replica ([ADR-0004](../../../docs/adr/0004-stateless-replicas-postgres.md): a cache that is safe to lose). It is not in PostgreSQL
//!   like the sign-in limits: those are about an account and must be the same on every replica; this is about the processors of the one that is asked, and a
//!   database write per debounced evaluation would compete for the pool that every request needs.
//!
//! Background work (the AI pipeline, the demo project at start) calls the engine without a slot: it is not a request of a user, and `jobs.maxConcurrent` bounds it.

use crate::auth::AuthUser;
use crate::error::ApiError;
use crate::metrics::Metrics;
use crate::state::AppState;
use axum::extract::{FromRequestParts, MatchedPath, Request, State};
use axum::http::request::Parts;
use axum::http::Method;
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use serde_json::json;
use std::collections::HashMap;
use std::convert::Infallible;
use std::sync::{Arc, Mutex};
use uuid::Uuid;

/// The routes the gate holds back: `(method, the route as the router names it)`. **The** list: the middleware reads it, and the test that walks it
/// (`heavy_endpoints_are_rate_limited`) is given a request for each entry, so a route cannot be gated in the test and not in the code, or the other way round.
/// A new route that runs the engine belongs here, and the test says which request to make for it.
pub const HEAVY_ROUTES: &[(&str, &str)] = &[
    ("POST", "/api/evaluate"),
    ("POST", "/api/recommendations"),
    ("POST", "/api/simulate"),
    ("POST", "/api/diff"),
    ("POST", "/api/ops/apply"),
    ("POST", "/api/placement/preview"),
    ("POST", "/api/placement/compare"),
    ("GET", "/api/projects/{id}/export/{format}"),
    ("POST", "/api/projects"),
    ("POST", "/api/projects/{id}/versions"),
    ("POST", "/api/projects/{id}/versions/{n}/restore"),
    ("POST", "/api/projects/{id}/waivers"),
    ("POST", "/api/proposals/{id}/apply"),
];

/// Is `method` on `route` one of the gated requests? A `HEAD` is the `GET` of the same route: the router answers it with the handler of the `GET`, which does all
/// of the work and throws the body away, so it costs what the `GET` costs.
pub fn is_heavy(method: &Method, route: &str) -> bool {
    let method = if method == Method::HEAD { Method::GET } else { method.clone() };
    HEAVY_ROUTES.iter().any(|(m, r)| method.as_str() == *m && route == *r)
}

#[derive(Default)]
struct Counts {
    all: usize,
    per_user: HashMap<Uuid, usize>,
}

struct Inner {
    counts: Mutex<Counts>,
    metrics: Arc<Metrics>,
}

/// What is running on this replica, in total and per user.
#[derive(Clone)]
pub struct HeavyGate(Arc<Inner>);

/// Which limit said no.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Busy {
    /// The user already has this many in flight.
    User(usize),
    /// The replica already has this many in flight, whoever's they are.
    Replica(usize),
}

struct Slot {
    gate: Arc<Inner>,
    user: Uuid,
}

impl Drop for Slot {
    fn drop(&mut self) {
        let mut c = self.gate.counts.lock().unwrap_or_else(|e| e.into_inner());
        c.all = c.all.saturating_sub(1);
        if let Some(n) = c.per_user.get_mut(&self.user) {
            *n = n.saturating_sub(1);
            if *n == 0 {
                c.per_user.remove(&self.user);
            }
        }
        self.gate.metrics.heavy_in_flight.set(c.all as i64);
    }
}

/// One slot of the gate. Cloning it shares the slot: it is given back when the last clone is dropped, so a handler can hand one to each blocking task it starts
/// and keep its own, and the slot stays taken until the processor is really free of the work.
#[derive(Clone)]
pub struct HeavyGuard(#[allow(dead_code)] Arc<Slot>);

impl HeavyGate {
    pub fn new(metrics: Arc<Metrics>) -> Self {
        HeavyGate(Arc::new(Inner { counts: Mutex::new(Counts::default()), metrics }))
    }

    /// A slot for `user`, or the limit that says no. `0` for a limit is no limit. The user's own limit is looked at first: it is the more useful thing to say.
    pub fn try_acquire(&self, user: Uuid, per_user: usize, per_replica: usize) -> Result<HeavyGuard, Busy> {
        let mut c = self.0.counts.lock().unwrap_or_else(|e| e.into_inner());
        if per_user > 0 && c.per_user.get(&user).copied().unwrap_or(0) >= per_user {
            self.0.metrics.heavy_rejected.with_label_values(&["user"]).inc();
            return Err(Busy::User(per_user));
        }
        if per_replica > 0 && c.all >= per_replica {
            self.0.metrics.heavy_rejected.with_label_values(&["replica"]).inc();
            return Err(Busy::Replica(per_replica));
        }
        c.all += 1;
        *c.per_user.entry(user).or_insert(0) += 1;
        self.0.metrics.heavy_in_flight.set(c.all as i64);
        Ok(HeavyGuard(Arc::new(Slot { gate: self.0.clone(), user })))
    }

    /// How many are running now (all users).
    pub fn in_flight(&self) -> usize {
        self.0.counts.lock().unwrap_or_else(|e| e.into_inner()).all
    }

    /// How many of `user`'s are running now.
    pub fn in_flight_of(&self, user: Uuid) -> usize {
        self.0.counts.lock().unwrap_or_else(|e| e.into_inner()).per_user.get(&user).copied().unwrap_or(0)
    }
}

/// What a handler holds while it works: the slot the gate took for the request, if it took one (it does for the routes in [`HEAVY_ROUTES`]; the AI pipeline
/// and the start-up code call the engine with none). Cheap to clone; give a clone to every closure that goes to the blocking pool.
#[derive(Clone, Default)]
pub struct Held(pub Option<HeavyGuard>);

impl<S: Send + Sync> FromRequestParts<S> for Held {
    type Rejection = Infallible;

    async fn from_request_parts(parts: &mut Parts, _: &S) -> Result<Self, Self::Rejection> {
        Ok(Held(parts.extensions.get::<HeavyGuard>().cloned()))
    }
}

/// The refusal: `429 rate_limited`, `Retry-After` from `limits.heavyRetryAfterSeconds`, and which limit it was in `details`.
fn refusal(busy: Busy, retry_after: u64) -> ApiError {
    let (scope, limit, message) = match busy {
        Busy::User(n) => ("user", n, format!("you already have {n} heavy requests running (evaluations, simulations, exports…): wait for one to finish")),
        Busy::Replica(n) => ("replica", n, format!("this server is busy with {n} heavy requests: try again shortly")),
    };
    ApiError::too_many("rate_limited", message).with_details(json!({"scope": scope, "limit": limit})).with_retry_after(retry_after)
}

/// The middleware: for a route in [`HEAVY_ROUTES`] it takes a slot for the signed-in user before the handler starts (and before the body is read) and puts it in the
/// request, where [`Held`] finds it; the slot is the request's own until a handler hands it on. A request with no valid session is not counted: the handler's
/// own extractor refuses it, in the shape every error has.
pub async fn gate(State(state): State<AppState>, req: Request, next: Next) -> Response {
    if !req.extensions().get::<MatchedPath>().is_some_and(|m| is_heavy(req.method(), m.as_str())) {
        return next.run(req).await;
    }
    let (mut parts, body) = req.into_parts();
    let who = AuthUser::from_request_parts(&mut parts, &state).await;
    let mut req = Request::from_parts(parts, body);
    let Ok(user) = who else { return next.run(req).await };
    let cfg = match state.cfg().await {
        Ok(cfg) => cfg,
        Err(e) => return e.into_response(),
    };
    let per_user = cfg.settings.num("limits.heavyConcurrentPerUser") as usize;
    let per_core = cfg.settings.num("limits.heavyConcurrentPerCore") as usize;
    let cores = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1);
    match state.heavy.try_acquire(user.id, per_user, per_core.saturating_mul(cores)) {
        Ok(guard) => {
            req.extensions_mut().insert(guard);
            next.run(req).await
        }
        Err(busy) => refusal(busy, cfg.settings.num("limits.heavyRetryAfterSeconds") as u64).into_response(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn gate() -> HeavyGate {
        HeavyGate::new(Arc::new(Metrics::new("test")))
    }

    #[test]
    fn a_user_has_at_most_so_many_and_other_users_are_not_affected() {
        let g = gate();
        let (ann, bob) = (Uuid::new_v4(), Uuid::new_v4());
        let held: Vec<_> = (0..3).map(|_| g.try_acquire(ann, 3, 0).expect("within the limit")).collect();
        assert_eq!(g.try_acquire(ann, 3, 0).err(), Some(Busy::User(3)));
        assert!(g.try_acquire(bob, 3, 0).is_ok(), "somebody else's slots are their own");
        assert_eq!((g.in_flight_of(ann), g.in_flight()), (3, 3), "the refused one and bob's dropped guard cost nothing");
        drop(held);
        assert_eq!(g.in_flight(), 0);
        assert!(g.try_acquire(ann, 3, 0).is_ok(), "slots come back");
    }

    #[test]
    fn the_replica_has_a_limit_of_its_own_and_zero_means_none() {
        let g = gate();
        let users: Vec<Uuid> = (0..5).map(|_| Uuid::new_v4()).collect();
        let held: Vec<_> = users[..4].iter().map(|u| g.try_acquire(*u, 3, 4).expect("room")).collect();
        assert_eq!(g.try_acquire(users[4], 3, 4).err(), Some(Busy::Replica(4)));
        assert!(g.try_acquire(users[4], 3, 0).is_ok(), "without a limit for the replica it is let in");
        drop(held);
        let one = Uuid::new_v4();
        let many: Vec<_> = (0..50).map(|_| g.try_acquire(one, 0, 0).expect("no limits")).collect();
        assert_eq!(g.in_flight_of(one), 50);
        drop(many);
        assert_eq!(g.in_flight_of(one), 0, "and the entry of the user is gone with the last slot");
    }

    #[test]
    fn a_slot_is_given_back_by_the_last_holder() {
        let g = gate();
        let u = Uuid::new_v4();
        let slot = g.try_acquire(u, 1, 0).unwrap();
        let shared = slot.clone();
        drop(slot);
        assert_eq!(g.in_flight(), 1, "a clone still holds it");
        assert!(g.try_acquire(u, 1, 0).is_err());
        drop(shared);
        assert_eq!(g.in_flight(), 0);
    }

    /// The reason the slot travels with the work: a request that is aborted drops its future, and the work on the blocking pool does not stop.
    #[tokio::test]
    async fn a_slot_moved_into_the_work_outlives_the_request_that_was_dropped() {
        let g = gate();
        let u = Uuid::new_v4();
        let held = Held(Some(g.try_acquire(u, 3, 0).unwrap()));
        let request = tokio::spawn(async move { crate::api_support::blocking(&held, || std::thread::sleep(Duration::from_millis(400))).await });
        tokio::time::sleep(Duration::from_millis(100)).await; // the work is on a blocking thread
        request.abort(); // what the editor does on the next edit
        let _ = request.await;
        assert_eq!(g.in_flight_of(u), 1, "the request is gone and the processor is still busy: the slot is still taken");
        for _ in 0..50 {
            if g.in_flight() == 0 {
                break;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        assert_eq!(g.in_flight(), 0, "and it comes back when the work ends");
    }

    #[test]
    fn the_list_names_each_route_once() {
        let mut seen = std::collections::HashSet::new();
        for (m, r) in HEAVY_ROUTES {
            assert!(seen.insert((*m, *r)), "{m} {r} twice");
            assert!(r.starts_with("/api/"), "{r}: the route as the router names it, with the prefix of the nest");
        }
        assert!(is_heavy(&Method::POST, "/api/evaluate"));
        assert!(!is_heavy(&Method::GET, "/api/evaluate"), "the method is part of the key");
        assert!(is_heavy(&Method::HEAD, "/api/projects/{id}/export/{format}"), "a HEAD runs the handler of the GET: it is as heavy");
        assert!(!is_heavy(&Method::HEAD, "/api/projects"), "and a HEAD of a route that is not heavy is not");
        assert!(!is_heavy(&Method::GET, "/api/projects"), "the list of projects is a query, not engine work");
    }
}
