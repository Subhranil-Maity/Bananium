//! The client itself: request construction, retry/backoff, and wiring the
//! rate limiter into every call.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use futures_util::future::join_all;
use reqwest::{Method, StatusCode};
use serde::Serialize;

use crate::error::{Error, Result};
use crate::facets::Facets;
use crate::models::{HashAlgorithm, Project, SearchResponse, Version, VersionFilesResponse};
use crate::ratelimit::RateLimiter;

/// Modrinth's real v2 API base, used by [`ModrinthClient::new`].
const DEFAULT_BASE_URL: &str = "https://api.modrinth.com/v2";

/// Total attempts (including the first) for a request that hits a 5xx, a
/// 429, or a transport error before this client gives up. Kept small on
/// purpose — papering over a real Modrinth outage indefinitely isn't this
/// client's job, just smoothing over the ordinary blips retrying helps
/// with.
const MAX_ATTEMPTS: u32 = 3;

/// How long to wait for a TCP/TLS connection before calling Modrinth
/// unreachable. Without this, a dead connection waits for the OS to give up
/// — about 21s on Windows — and that happens again on every retry.
pub const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);

/// The most one request (connect, send, and read the whole body) may take.
/// Modrinth's API answers in well under a second when healthy, so 30s only
/// ever cuts off a stalled connection, which would otherwise hang the
/// command that made it forever.
pub const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);

/// Per-file GET lookups run at most this many at a time when a bulk POST
/// endpoint is blocked (see [`ModrinthClient::version_files`]).
const FALLBACK_CONCURRENCY: usize = 8;

/// Rate-limit waits shorter than this aren't reported to the
/// [`RetryObserver`]: they pass before a user would notice anything.
const NOTICEABLE_WAIT: Duration = Duration::from_secs(1);

/// Why a request is being retried (or held back), as reported to a
/// [`RetryObserver`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RetryReason {
    /// The previous attempt hit [`REQUEST_TIMEOUT`].
    Timeout,
    /// The previous attempt couldn't connect.
    Connect,
    /// Some other transport failure (reset connection, unreadable body).
    Transport,
    /// Modrinth answered with this 5xx status.
    ServerError(u16),
    /// Modrinth's rate limit is used up; waiting for the window to reset.
    RateLimited,
}

impl std::fmt::Display for RetryReason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RetryReason::Timeout => write!(f, "Modrinth didn't respond"),
            RetryReason::Connect => write!(f, "couldn't connect to Modrinth"),
            RetryReason::Transport => write!(f, "the connection to Modrinth failed"),
            RetryReason::ServerError(status) => write!(f, "Modrinth returned an error ({status})"),
            RetryReason::RateLimited => write!(f, "Modrinth's rate limit was reached"),
        }
    }
}

/// One retry (or rate-limit wait) about to happen, for a [`RetryObserver`].
#[derive(Debug, Clone)]
pub struct RetryNotice {
    /// The API path without the host, e.g. `search` or `version_files`.
    pub endpoint: String,
    /// The attempt about to be made (2 is the first retry), out of
    /// `max_attempts`.
    pub attempt: u32,
    pub max_attempts: u32,
    pub reason: RetryReason,
    /// How long the client waits before that attempt.
    pub wait: Duration,
}

/// Called on every retry and every noticeable rate-limit wait, so the
/// caller can show "retrying" instead of a request that seems stuck.
pub type RetryObserver = Arc<dyn Fn(&RetryNotice) + Send + Sync>;

/// A typed async client for the Modrinth v2 REST API
/// (`api.modrinth.com/v2`). Holds one shared `reqwest::Client` and the
/// rate-limit state Modrinth reports on every response, so callers never
/// have to pace requests themselves or handle a 429 by hand.
pub struct ModrinthClient {
    http: reqwest::Client,
    base_url: String,
    limiter: RateLimiter,
    observer: Option<RetryObserver>,
    /// Set once Modrinth's firewall has refused a bulk POST, so later bulk
    /// lookups go straight to the GET fallback instead of being refused
    /// again first.
    posts_blocked: AtomicBool,
}

impl ModrinthClient {
    /// Build a client against the real Modrinth API. `user_agent` should
    /// follow Modrinth's required `<name>/<version> (<contact>)` form —
    /// Modrinth aggressively rate-limits generic or missing user agents,
    /// so this isn't optional decoration.
    pub fn new(user_agent: &str) -> Result<Self> {
        Self::with_base_url(user_agent, DEFAULT_BASE_URL)
    }

    /// As [`ModrinthClient::new`], but against an arbitrary base URL.
    /// Exists so tests can point this client at a local mock server
    /// instead of the real API; production callers should use
    /// [`ModrinthClient::new`].
    pub fn with_base_url(user_agent: &str, base_url: impl Into<String>) -> Result<Self> {
        Self::with_timeouts(user_agent, base_url, CONNECT_TIMEOUT, REQUEST_TIMEOUT)
    }

    /// As [`ModrinthClient::with_base_url`], with explicit timeouts (see
    /// [`CONNECT_TIMEOUT`] and [`REQUEST_TIMEOUT`] for the defaults) — so a
    /// test can exercise a timeout without waiting 30 seconds.
    pub fn with_timeouts(
        user_agent: &str,
        base_url: impl Into<String>,
        connect: Duration,
        request: Duration,
    ) -> Result<Self> {
        let http = reqwest::Client::builder()
            .user_agent(user_agent)
            .connect_timeout(connect)
            .timeout(request)
            .build()?;
        Ok(Self {
            http,
            base_url: base_url.into(),
            limiter: RateLimiter::new(),
            observer: None,
            posts_blocked: AtomicBool::new(false),
        })
    }

    /// Report every retry and noticeable rate-limit wait to `observer`.
    pub fn with_retry_observer(mut self, observer: RetryObserver) -> Self {
        self.observer = Some(observer);
        self
    }

    fn notify(&self, notice: RetryNotice) {
        if let Some(observer) = &self.observer {
            observer(&notice);
        }
    }

    /// `GET /search` — full-text project search with facet filters,
    /// pagination, and a sort order.
    pub async fn search(&self, query: &SearchQuery) -> Result<SearchResponse> {
        let mut params: Vec<(String, String)> = Vec::new();
        if let Some(q) = &query.query {
            params.push(("query".to_string(), q.clone()));
        }
        if let Some(facets) = &query.facets {
            if !facets.is_empty() {
                params.push(("facets".to_string(), facets.to_query_value()));
            }
        }
        params.push(("index".to_string(), query.index.as_str().to_string()));
        params.push(("offset".to_string(), query.offset.to_string()));
        params.push(("limit".to_string(), query.limit.to_string()));
        self.request(Method::GET, "search", &params, None::<&()>)
            .await
    }

    /// `GET /project/{id|slug}` — look up a single project by its base62
    /// id or vanity slug (Modrinth accepts either interchangeably at this
    /// endpoint).
    pub async fn project(&self, id_or_slug: &str) -> Result<Project> {
        self.request(
            Method::GET,
            &format!("project/{id_or_slug}"),
            &[],
            None::<&()>,
        )
        .await
    }

    /// `GET /projects?ids=[...]` — many projects in one request, in no
    /// particular order; unknown ids are simply absent. Use this instead of
    /// a [`ModrinthClient::project`] loop, which is one request each against
    /// a 300-requests-per-minute budget.
    pub async fn projects(&self, ids: &[String]) -> Result<Vec<Project>> {
        if ids.is_empty() {
            return Ok(Vec::new());
        }
        let params = [("ids".to_string(), json_string_array(ids))];
        self.request(Method::GET, "projects", &params, None::<&()>)
            .await
    }

    /// `GET /project/{id|slug}/version` — a project's versions, optionally
    /// narrowed to those matching `filter`.
    pub async fn project_versions(
        &self,
        id_or_slug: &str,
        filter: &VersionsFilter,
    ) -> Result<Vec<Version>> {
        let mut params: Vec<(String, String)> = Vec::new();
        if let Some(loaders) = &filter.loaders {
            params.push(("loaders".to_string(), json_string_array(loaders)));
        }
        if let Some(game_versions) = &filter.game_versions {
            params.push((
                "game_versions".to_string(),
                json_string_array(game_versions),
            ));
        }
        if let Some(featured) = filter.featured {
            params.push(("featured".to_string(), featured.to_string()));
        }
        self.request(
            Method::GET,
            &format!("project/{id_or_slug}/version"),
            &params,
            None::<&()>,
        )
        .await
    }

    /// `GET /version_file/{hash}` — the version one file belongs to, or
    /// `None` if Modrinth doesn't know it (a 404).
    pub async fn version_file(
        &self,
        hash: &str,
        algorithm: HashAlgorithm,
    ) -> Result<Option<Version>> {
        let params = [("algorithm".to_string(), algorithm.as_str().to_string())];
        match self
            .request(
                Method::GET,
                &format!("version_file/{hash}"),
                &params,
                None::<&()>,
            )
            .await
        {
            Ok(version) => Ok(Some(version)),
            Err(Error::Status { status: 404, .. }) => Ok(None),
            Err(err) => Err(err),
        }
    }

    /// `POST /version_files` — bulk hash → version lookup, used to
    /// identify jars already on disk without one request per file.
    ///
    /// Modrinth's firewall sometimes refuses POSTs from a network outright
    /// (a 403 "Request blocked" page, whatever the request) while GETs
    /// still work. Then this falls back to one [`ModrinthClient::version_file`]
    /// GET per hash — slower, but the same answer.
    pub async fn version_files(
        &self,
        hashes: &[String],
        algorithm: HashAlgorithm,
    ) -> Result<VersionFilesResponse> {
        #[derive(Serialize)]
        struct Body<'a> {
            hashes: &'a [String],
            algorithm: HashAlgorithm,
        }
        if !self.posts_blocked.load(Ordering::Relaxed) {
            match self
                .request(
                    Method::POST,
                    "version_files",
                    &[],
                    Some(&Body { hashes, algorithm }),
                )
                .await
            {
                Err(Error::Status { status: 403, .. }) => self.note_posts_blocked("version_files"),
                other => return other,
            }
        }
        self.lookup_each(hashes, algorithm).await
    }

    /// `POST /version_files/update` — bulk update check: given hashes plus
    /// loader/game-version constraints, returns the latest matching
    /// version per hash. Falls back to GETs like
    /// [`ModrinthClient::version_files`] when POSTs are blocked: each file's
    /// project, then that project's versions under the same constraints.
    pub async fn update_version_files(
        &self,
        request: &UpdateVersionFilesRequest,
    ) -> Result<VersionFilesResponse> {
        if !self.posts_blocked.load(Ordering::Relaxed) {
            match self
                .request(Method::POST, "version_files/update", &[], Some(request))
                .await
            {
                Err(Error::Status { status: 403, .. }) => {
                    self.note_posts_blocked("version_files/update")
                }
                other => return other,
            }
        }
        let current = self.lookup_each(&request.hashes, request.algorithm).await?;
        let filter = VersionsFilter {
            loaders: Some(request.loaders.clone()).filter(|l| !l.is_empty()),
            game_versions: Some(request.game_versions.clone()).filter(|g| !g.is_empty()),
            featured: None,
        };
        let current: Vec<(String, Version)> = current.into_iter().collect();
        let mut latest = HashMap::new();
        for batch in current.chunks(FALLBACK_CONCURRENCY) {
            let lookups = batch
                .iter()
                .map(|(_, version)| self.project_versions(&version.project_id, &filter));
            for ((hash, _), versions) in batch.iter().zip(join_all(lookups).await) {
                // Modrinth lists newest first.
                let newest = versions?.into_iter().find(|v| {
                    request
                        .version_types
                        .as_ref()
                        .is_none_or(|types| types.contains(&v.version_type))
                });
                if let Some(v) = newest {
                    latest.insert(hash.clone(), v);
                }
            }
        }
        Ok(latest)
    }

    fn note_posts_blocked(&self, endpoint: &str) {
        if !self.posts_blocked.swap(true, Ordering::Relaxed) {
            tracing::warn!(
                endpoint,
                "Modrinth's firewall refused a POST request (403); using per-file GET lookups instead"
            );
        }
    }

    /// One `GET /version_file/{hash}` per hash, a few at a time; unknown
    /// hashes are left out, exactly like the bulk endpoint does.
    async fn lookup_each(
        &self,
        hashes: &[String],
        algorithm: HashAlgorithm,
    ) -> Result<VersionFilesResponse> {
        let mut found = HashMap::new();
        for batch in hashes.chunks(FALLBACK_CONCURRENCY) {
            let lookups = batch.iter().map(|hash| self.version_file(hash, algorithm));
            for (hash, version) in batch.iter().zip(join_all(lookups).await) {
                if let Some(version) = version? {
                    found.insert(hash.clone(), version);
                }
            }
        }
        Ok(found)
    }

    /// The shared request lifecycle behind every endpoint above:
    /// preemptive rate-limit pacing, JSON encode/decode, and retry with
    /// exponential backoff on a transport error or 5xx, or Modrinth's
    /// requested wait on a 429. A non-retryable 4xx (anything but 429)
    /// returns immediately rather than burning through attempts on a
    /// request that will never succeed.
    async fn request<T, B>(
        &self,
        method: Method,
        path: &str,
        query: &[(String, String)],
        body: Option<&B>,
    ) -> Result<T>
    where
        T: serde::de::DeserializeOwned,
        B: Serialize,
    {
        let url = format!("{}/{path}", self.base_url);
        // The path alone (no ids or query) keeps log lines short and groups
        // retries by endpoint.
        let endpoint = match path.split('/').collect::<Vec<_>>().as_slice() {
            ["project", _, "version"] => "project/version".to_string(),
            ["project", _] => "project".to_string(),
            ["version_file", _] => "version_file".to_string(),
            _ => path.to_string(),
        };
        let mut last_err: Option<Error> = None;
        let mut last_reason = RetryReason::Transport;

        for attempt in 0..MAX_ATTEMPTS {
            if attempt > 0 {
                // 200ms, 400ms, ... — a couple of quick retries is enough
                // to ride out a blip; anything longer and the caller is
                // better served by the error surfacing.
                let backoff = Duration::from_millis(200 * 2u64.pow(attempt - 1));
                let error = last_err
                    .as_ref()
                    .map(ToString::to_string)
                    .unwrap_or_default();
                tracing::warn!(
                    endpoint = %endpoint,
                    attempt = attempt + 1,
                    max_attempts = MAX_ATTEMPTS,
                    reason = %last_reason,
                    ?backoff,
                    "retrying modrinth request: {error}"
                );
                self.notify(RetryNotice {
                    endpoint: endpoint.clone(),
                    attempt: attempt + 1,
                    max_attempts: MAX_ATTEMPTS,
                    reason: last_reason,
                    wait: backoff,
                });
                tokio::time::sleep(backoff).await;
            }

            if let Some(wait) = self.limiter.pending_wait() {
                tracing::info!(endpoint = %endpoint, ?wait, "modrinth rate limit used up, waiting for it to reset");
                if wait >= NOTICEABLE_WAIT {
                    self.notify(RetryNotice {
                        endpoint: endpoint.clone(),
                        attempt: attempt + 1,
                        max_attempts: MAX_ATTEMPTS,
                        reason: RetryReason::RateLimited,
                        wait,
                    });
                }
                tokio::time::sleep(wait).await;
            }

            let mut req = self.http.request(method.clone(), &url);
            if !query.is_empty() {
                req = req.query(query);
            }
            if let Some(body) = body {
                req = req.json(body);
            }

            let started = Instant::now();
            let response = match req.send().await {
                Ok(response) => response,
                Err(err) => {
                    (last_reason, last_err) = classify(&url, err);
                    continue;
                }
            };

            self.limiter.update_from_headers(response.headers());
            let status = response.status();

            if status.is_success() {
                // Reading the body can stall or reset just like the
                // request itself, so it's retried the same way.
                let bytes = match response.bytes().await {
                    Ok(bytes) => bytes,
                    Err(err) => {
                        (last_reason, last_err) = classify(&url, err);
                        continue;
                    }
                };
                tracing::debug!(
                    endpoint = %endpoint,
                    %status,
                    ms = started.elapsed().as_millis() as u64,
                    bytes = bytes.len(),
                    "modrinth request"
                );
                return serde_json::from_slice(&bytes).map_err(|source| Error::Json {
                    url: url.clone(),
                    source,
                });
            }
            tracing::debug!(
                endpoint = %endpoint,
                %status,
                ms = started.elapsed().as_millis() as u64,
                "modrinth request"
            );

            if status == StatusCode::TOO_MANY_REQUESTS {
                let wait = retry_after(&response).unwrap_or(Duration::from_secs(1));
                tracing::warn!(endpoint = %endpoint, ?wait, "modrinth rate limit hit (429), waiting");
                if wait >= NOTICEABLE_WAIT {
                    self.notify(RetryNotice {
                        endpoint: endpoint.clone(),
                        attempt: attempt + 1,
                        max_attempts: MAX_ATTEMPTS,
                        reason: RetryReason::RateLimited,
                        wait,
                    });
                }
                tokio::time::sleep(wait).await;
                last_reason = RetryReason::RateLimited;
                last_err = Some(Error::Status {
                    status: status.as_u16(),
                    url: url.clone(),
                });
                continue;
            }

            if status.is_server_error() {
                last_reason = RetryReason::ServerError(status.as_u16());
                last_err = Some(Error::Status {
                    status: status.as_u16(),
                    url: url.clone(),
                });
                continue;
            }

            // Any other non-2xx (typically a 4xx) will never succeed on
            // retry — fail immediately instead of burning attempts.
            return Err(Error::Status {
                status: status.as_u16(),
                url: url.clone(),
            });
        }

        Err(last_err.unwrap_or(Error::RetriesExhausted { url }))
    }
}

/// Turn a transport failure into the reason it's retried for and the error
/// reported if it's the last one: timeouts and failed connects get their
/// own variants, so a frontend can say "Modrinth didn't respond".
fn classify(url: &str, err: reqwest::Error) -> (RetryReason, Option<Error>) {
    if err.is_timeout() {
        (
            RetryReason::Timeout,
            Some(Error::Timeout {
                url: url.to_string(),
            }),
        )
    } else if err.is_connect() {
        (
            RetryReason::Connect,
            Some(Error::Unreachable {
                url: url.to_string(),
                source: err,
            }),
        )
    } else {
        (RetryReason::Transport, Some(Error::Http(err)))
    }
}

/// `Retry-After` as Modrinth sends it: an integer number of seconds. `None`
/// if the header is absent or not a plain integer (Modrinth doesn't use
/// the HTTP-date form of this header).
fn retry_after(response: &reqwest::Response) -> Option<Duration> {
    let value = response.headers().get(reqwest::header::RETRY_AFTER)?;
    value
        .to_str()
        .ok()?
        .parse::<u64>()
        .ok()
        .map(Duration::from_secs)
}

/// Modrinth encodes array-valued query params (`loaders`, `game_versions`,
/// ...) the same way it encodes `facets`: a JSON array as a string.
fn json_string_array(values: &[String]) -> String {
    serde_json::to_string(values).expect("Vec<String> always serializes")
}

/// Sort order for [`ModrinthClient::search`] results (the `index` query
/// param — named `Sort` here since that's what it actually controls).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Sort {
    #[default]
    Relevance,
    Downloads,
    Follows,
    Newest,
    Updated,
}

impl Sort {
    fn as_str(self) -> &'static str {
        match self {
            Sort::Relevance => "relevance",
            Sort::Downloads => "downloads",
            Sort::Follows => "follows",
            Sort::Newest => "newest",
            Sort::Updated => "updated",
        }
    }
}

/// Parameters for [`ModrinthClient::search`]. Construct with
/// [`SearchQuery::new`] and chain the setters that apply — every field has
/// a sensible default matching Modrinth's own (`index: relevance`,
/// `offset: 0`, `limit: 10`).
#[derive(Debug, Clone)]
pub struct SearchQuery {
    query: Option<String>,
    facets: Option<Facets>,
    index: Sort,
    offset: u32,
    limit: u32,
}

impl Default for SearchQuery {
    fn default() -> Self {
        Self {
            query: None,
            facets: None,
            index: Sort::default(),
            offset: 0,
            limit: 10,
        }
    }
}

impl SearchQuery {
    /// A query matching Modrinth's own defaults: no search text, no
    /// facets, sorted by relevance, first 10 results.
    pub fn new() -> Self {
        Self::default()
    }

    /// Free-text search terms.
    pub fn query(mut self, query: impl Into<String>) -> Self {
        self.query = Some(query.into());
        self
    }

    /// Facet filters built via [`crate::FacetsBuilder`].
    pub fn facets(mut self, facets: Facets) -> Self {
        self.facets = Some(facets);
        self
    }

    /// Sort order.
    pub fn index(mut self, index: Sort) -> Self {
        self.index = index;
        self
    }

    /// Results to skip, for pagination.
    pub fn offset(mut self, offset: u32) -> Self {
        self.offset = offset;
        self
    }

    /// Results per page (Modrinth caps this at 100).
    pub fn limit(mut self, limit: u32) -> Self {
        self.limit = limit;
        self
    }
}

/// Optional filters for [`ModrinthClient::project_versions`]. All `None`
/// by default, meaning "every version of this project."
#[derive(Debug, Clone, Default)]
pub struct VersionsFilter {
    /// Only versions supporting at least one of these loaders.
    pub loaders: Option<Vec<String>>,
    /// Only versions supporting at least one of these game versions.
    pub game_versions: Option<Vec<String>>,
    /// Only featured (`Some(true)`) or only non-featured (`Some(false)`)
    /// versions; `None` returns both.
    pub featured: Option<bool>,
}

/// Request body for `POST /version_files/update`: for each hash, ask
/// Modrinth for the latest version compatible with the given loaders and
/// game versions — the shape a mod-update check needs ("is there a newer
/// version *I can actually use*"), not just "does a newer version exist at
/// all."
#[derive(Debug, Clone, Serialize)]
pub struct UpdateVersionFilesRequest {
    hashes: Vec<String>,
    algorithm: HashAlgorithm,
    loaders: Vec<String>,
    game_versions: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    version_types: Option<Vec<String>>,
}

impl UpdateVersionFilesRequest {
    /// `hashes` are looked up in `algorithm` form; `loaders` and
    /// `game_versions` constrain which version counts as "latest" for each
    /// hash.
    pub fn new(
        hashes: Vec<String>,
        algorithm: HashAlgorithm,
        loaders: Vec<String>,
        game_versions: Vec<String>,
    ) -> Self {
        Self {
            hashes,
            algorithm,
            loaders,
            game_versions,
            version_types: None,
        }
    }

    /// Narrow matches to specific release channels (e.g. only `"release"`)
    /// rather than accepting any.
    pub fn version_types(mut self, version_types: Vec<String>) -> Self {
        self.version_types = Some(version_types);
        self
    }
}
