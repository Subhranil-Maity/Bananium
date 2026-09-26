//! The client itself: request construction, retry/backoff, and wiring the
//! rate limiter into every call.

use std::time::Duration;

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

/// A typed async client for the Modrinth v2 REST API
/// (`api.modrinth.com/v2`). Holds one shared `reqwest::Client` and the
/// rate-limit state Modrinth reports on every response, so callers never
/// have to pace requests themselves or handle a 429 by hand.
pub struct ModrinthClient {
    http: reqwest::Client,
    base_url: String,
    limiter: RateLimiter,
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
        let http = reqwest::Client::builder().user_agent(user_agent).build()?;
        Ok(Self {
            http,
            base_url: base_url.into(),
            limiter: RateLimiter::new(),
        })
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

    /// `POST /version_files` — bulk hash → version lookup, used to
    /// identify jars already on disk (e.g. `mod adopt`) without one
    /// request per file.
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
        self.request(
            Method::POST,
            "version_files",
            &[],
            Some(&Body { hashes, algorithm }),
        )
        .await
    }

    /// `POST /version_files/update` — bulk update check: given hashes plus
    /// loader/game-version constraints, returns the latest matching
    /// version per hash.
    pub async fn update_version_files(
        &self,
        request: &UpdateVersionFilesRequest,
    ) -> Result<VersionFilesResponse> {
        self.request(Method::POST, "version_files/update", &[], Some(request))
            .await
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
        let mut last_err: Option<Error> = None;

        for attempt in 0..MAX_ATTEMPTS {
            if attempt > 0 {
                // 200ms, 400ms, ... — a couple of quick retries is enough
                // to ride out a blip; anything longer and the caller is
                // better served by the error surfacing.
                let backoff = Duration::from_millis(200 * 2u64.pow(attempt - 1));
                tracing::warn!(url = %url, attempt, ?backoff, "retrying modrinth request");
                tokio::time::sleep(backoff).await;
            }

            self.limiter.wait_if_exhausted().await;

            let mut req = self.http.request(method.clone(), &url);
            if !query.is_empty() {
                req = req.query(query);
            }
            if let Some(body) = body {
                req = req.json(body);
            }

            let response = match req.send().await {
                Ok(response) => response,
                Err(err) => {
                    last_err = Some(Error::Http(err));
                    continue;
                }
            };

            self.limiter.update_from_headers(response.headers());
            let status = response.status();
            tracing::debug!(url = %url, %status, "modrinth request");

            if status.is_success() {
                let bytes = response.bytes().await.map_err(Error::Http)?;
                return serde_json::from_slice(&bytes).map_err(|source| Error::Json {
                    url: url.clone(),
                    source,
                });
            }

            if status == StatusCode::TOO_MANY_REQUESTS {
                let wait = retry_after(&response).unwrap_or(Duration::from_secs(1));
                tracing::warn!(url = %url, ?wait, "modrinth rate limit hit (429), waiting");
                tokio::time::sleep(wait).await;
                last_err = Some(Error::Status {
                    status: status.as_u16(),
                    url: url.clone(),
                });
                continue;
            }

            if status.is_server_error() {
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
