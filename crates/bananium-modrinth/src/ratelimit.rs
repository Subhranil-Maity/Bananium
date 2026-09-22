//! Preemptive client-side rate limiting against Modrinth's `X-Ratelimit-*`
//! response headers. Modrinth caps every client at 300 requests/minute;
//! rather than only reacting after a 429 is already hit, this tracks the
//! most recently reported `remaining`/`reset` pair and, once `remaining`
//! reaches zero, makes the *next* request wait out the reset window before
//! it goes out at all — so a caller issuing a burst of requests degrades
//! into pacing instead of a wall of 429s.

use std::sync::Mutex;
use std::time::{Duration, Instant};

use reqwest::header::HeaderMap;

/// Rate-limit state derived from the most recent response, if any have
/// come back yet.
struct State {
    remaining: u32,
    /// Wall-clock instant the current window resets, computed from the
    /// `X-Ratelimit-Reset` header (seconds-until-reset) at the moment that
    /// response was received.
    reset_at: Instant,
}

/// Tracks Modrinth's rate-limit headers across requests made through one
/// [`crate::ModrinthClient`]. Not a full token bucket — it only guards the
/// one case that matters here: don't send a request known in advance to
/// draw a 429.
pub(crate) struct RateLimiter {
    state: Mutex<Option<State>>,
}

impl RateLimiter {
    pub(crate) fn new() -> Self {
        Self {
            state: Mutex::new(None),
        }
    }

    /// Record the rate-limit headers from a response. A response missing
    /// either header (e.g. some error responses don't carry them) leaves
    /// the previous state untouched rather than clearing it — losing track
    /// of a real exhausted window would defeat the point of this guard.
    pub(crate) fn update_from_headers(&self, headers: &HeaderMap) {
        let remaining = header_u32(headers, "x-ratelimit-remaining");
        let reset_secs = header_u32(headers, "x-ratelimit-reset");
        if let (Some(remaining), Some(reset_secs)) = (remaining, reset_secs) {
            let reset_at = Instant::now() + Duration::from_secs(u64::from(reset_secs));
            *self.state.lock().unwrap() = Some(State {
                remaining,
                reset_at,
            });
        }
    }

    /// Block until it's safe to send another request. A no-op unless the
    /// last response reported zero remaining and its reset window hasn't
    /// elapsed yet, in which case this sleeps for exactly what's left of
    /// it.
    pub(crate) async fn wait_if_exhausted(&self) {
        let wait_for = {
            let guard = self.state.lock().unwrap();
            guard.as_ref().and_then(|state| {
                if state.remaining == 0 {
                    Some(state.reset_at.saturating_duration_since(Instant::now()))
                } else {
                    None
                }
            })
        };
        if let Some(wait_for) = wait_for {
            if !wait_for.is_zero() {
                tracing::debug!(
                    ?wait_for,
                    "modrinth rate limit exhausted, waiting for reset"
                );
                tokio::time::sleep(wait_for).await;
            }
        }
    }
}

fn header_u32(headers: &HeaderMap, name: &str) -> Option<u32> {
    headers.get(name)?.to_str().ok()?.parse().ok()
}
