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

    /// How long to wait before it's safe to send another request: `None`
    /// unless the last response reported zero remaining and its reset
    /// window hasn't elapsed yet, in which case exactly what's left of it.
    /// The caller sleeps (and reports the wait), since only it knows which
    /// request is being held back.
    pub(crate) fn pending_wait(&self) -> Option<Duration> {
        let guard = self.state.lock().unwrap();
        guard
            .as_ref()
            .filter(|state| state.remaining == 0)
            .map(|state| state.reset_at.saturating_duration_since(Instant::now()))
            .filter(|wait| !wait.is_zero())
    }
}

fn header_u32(headers: &HeaderMap, name: &str) -> Option<u32> {
    headers.get(name)?.to_str().ok()?.parse().ok()
}
