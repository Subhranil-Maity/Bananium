//! A typed async client for the Modrinth v2 REST API
//! (`api.modrinth.com/v2`): `GET /search` with a typed facet builder,
//! `GET /project/{id|slug}` and `GET /project/{id|slug}/version`,
//! `POST /version_files` and `POST /version_files/update` for bulk hash
//! lookups. Requests carry a compliant `User-Agent` (Modrinth rate-limits
//! generic ones), pace themselves preemptively off the `X-Ratelimit-*`
//! response headers instead of only reacting to a 429, and retry with
//! backoff on a 429 or a 5xx.
//!
//! This crate is deliberately only the HTTP layer: it does **not** include
//! the SQLite/FTS5 offline mirror, the mod lockfile, or dependency
//! resolution that the rest of PLAN.md's M4 describes — those build on top
//! of this client and live elsewhere.

mod client;
mod error;
mod facets;
mod models;
mod ratelimit;

pub use client::{ModrinthClient, SearchQuery, Sort, UpdateVersionFilesRequest, VersionsFilter};
pub use error::{Error, Result};
pub use facets::{Facet, Facets, FacetsBuilder};
pub use models::{
    DonationUrl, FileHashes, GalleryImage, HashAlgorithm, License, Project, SearchHit,
    SearchResponse, Version, VersionDependency, VersionFile, VersionFilesResponse,
};
