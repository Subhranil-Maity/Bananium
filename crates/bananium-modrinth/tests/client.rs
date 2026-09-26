//! Integration tests against a local `wiremock` server, covering all HTTP
//! behavior including rate-limit headers and 5xx retry. Every test uses short synthetic durations rather
//! than the real 60s rate-limit window, so the suite stays fast.

use std::sync::atomic::{AtomicUsize, Ordering};

use bananium_modrinth::{
    Facet, FacetsBuilder, HashAlgorithm, ModrinthClient, RetryNotice, RetryReason, SearchQuery,
    UpdateVersionFilesRequest, VersionsFilter,
};
use wiremock::matchers::{body_json, method, path, query_param};
use wiremock::{Mock, MockServer, Request, Respond, ResponseTemplate};

const TEST_USER_AGENT: &str = "bananium-test/0.0.0 (https://example.invalid/bananium)";

async fn client_for(server: &MockServer) -> ModrinthClient {
    ModrinthClient::with_base_url(TEST_USER_AGENT, server.uri()).expect("client builds")
}

/// A [`Respond`] that returns a different canned response on each
/// successive call to the same mock, used to simulate "fails once, then
/// succeeds" without wiremock's declarative matchers (which match by
/// request shape, not by call count).
struct Sequence {
    calls: AtomicUsize,
    responses: Vec<ResponseTemplate>,
}

impl Sequence {
    fn new(responses: Vec<ResponseTemplate>) -> Self {
        Self {
            calls: AtomicUsize::new(0),
            responses,
        }
    }
}

impl Respond for Sequence {
    fn respond(&self, _request: &Request) -> ResponseTemplate {
        let call = self.calls.fetch_add(1, Ordering::SeqCst);
        let idx = call.min(self.responses.len() - 1);
        self.responses[idx].clone()
    }
}

fn sample_version_json(id: &str) -> serde_json::Value {
    serde_json::json!({
        "id": id,
        "project_id": "AABBCCDD",
        "author_id": "user1",
        "name": "Version 1.0",
        "version_number": "1.0.0",
        "changelog": "Initial release",
        "date_published": "2024-01-01T00:00:00Z",
        "downloads": 42,
        "version_type": "release",
        "status": "listed",
        "featured": true,
        "files": [{
            "hashes": {"sha1": "abc123", "sha512": "def456"},
            "url": "https://cdn.modrinth.com/data/AABBCCDD/versions/1.0.0/mod.jar",
            "filename": "mod.jar",
            "primary": true,
            "size": 1024,
            "file_type": null,
        }],
        "dependencies": [],
        "game_versions": ["1.21.1"],
        "loaders": ["fabric"],
        "environment": "client_and_server",
    })
}

#[tokio::test]
async fn search_round_trip_sends_exact_facets_json_and_parses_response() {
    let server = MockServer::start().await;

    let expected_facets = r#"[["categories:fabric"],["versions:1.21.1"],["project_type:mod"]]"#;

    Mock::given(method("GET"))
        .and(path("/search"))
        .and(query_param("query", "sodium"))
        .and(query_param("facets", expected_facets))
        .and(query_param("index", "downloads"))
        .and(query_param("offset", "0"))
        .and(query_param("limit", "20"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "hits": [{
                "project_id": "AABBCCDD",
                "project_type": "mod",
                "slug": "sodium",
                "title": "Sodium",
                "description": "A modern rendering engine",
                "author": "jellysquid3",
                "categories": ["optimization"],
                "display_categories": ["optimization"],
                "versions": ["1.21.1"],
                "downloads": 1000000,
                "follows": 5000,
                "icon_url": null,
                "date_created": "2020-01-01T00:00:00Z",
                "date_modified": "2024-01-01T00:00:00Z",
                "latest_version": "1.21.1",
                "license": "LGPL-3.0",
                "gallery": [],
                "color": null,
            }],
            "offset": 0,
            "limit": 20,
            "total_hits": 1,
        })))
        .expect(1)
        .mount(&server)
        .await;

    let client = client_for(&server).await;

    let facets = FacetsBuilder::new()
        .and(Facet::loader("fabric"))
        .and(Facet::version("1.21.1"))
        .and(Facet::project_type("mod"))
        .build();

    let query = SearchQuery::new()
        .query("sodium")
        .facets(facets)
        .index(bananium_modrinth::Sort::Downloads)
        .offset(0)
        .limit(20);

    let result = client.search(&query).await.expect("search succeeds");

    assert_eq!(result.total_hits, 1);
    assert_eq!(result.hits.len(), 1);
    assert_eq!(result.hits[0].title, "Sodium");
    assert_eq!(result.hits[0].project_id, "AABBCCDD");
}

#[tokio::test]
async fn project_lookup_by_slug() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/project/sodium"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "id": "AABBCCDD",
            "team": "team1",
            "slug": "sodium",
            "title": "Sodium",
            "description": "A modern rendering engine",
            "body": "Full description here",
            "status": "approved",
            "project_type": "mod",
            "categories": ["optimization"],
            "additional_categories": [],
            "game_versions": ["1.21.1"],
            "loaders": ["fabric"],
            "versions": ["v1"],
            "license": {"id": "LGPL-3.0", "name": "GNU LGPL v3", "url": null},
            "published": "2020-01-01T00:00:00Z",
            "updated": "2024-01-01T00:00:00Z",
            "downloads": 1000000,
            "followers": 5000,
            "gallery": [],
            "icon_url": null,
            "donation_urls": [],
        })))
        .expect(1)
        .mount(&server)
        .await;

    let client = client_for(&server).await;
    let project = client
        .project("sodium")
        .await
        .expect("project lookup succeeds");

    assert_eq!(project.id, "AABBCCDD");
    assert_eq!(project.title, "Sodium");
    assert_eq!(project.license.unwrap().id, "LGPL-3.0");
}

#[tokio::test]
async fn projects_batch_lookup_sends_ids_as_a_json_array() {
    let server = MockServer::start().await;
    let project = |id: &str, title: &str| {
        serde_json::json!({
            "id": id, "team": "t", "slug": null, "title": title, "description": "",
            "body": "", "status": "approved", "project_type": "mod", "categories": [],
            "additional_categories": [], "game_versions": [], "loaders": [], "versions": [],
            "license": null, "published": "2020-01-01T00:00:00Z",
            "updated": "2024-01-01T00:00:00Z", "downloads": 0, "followers": 0,
            "gallery": [], "icon_url": null, "donation_urls": [],
        })
    };
    Mock::given(method("GET"))
        .and(path("/projects"))
        .and(query_param("ids", r#"["AA","BB"]"#))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(vec![project("AA", "Sodium"), project("BB", "Iris")]),
        )
        .expect(1)
        .mount(&server)
        .await;

    let client = client_for(&server).await;
    let projects = client
        .projects(&["AA".to_string(), "BB".to_string()])
        .await
        .expect("batch lookup succeeds");
    assert_eq!(projects.len(), 2);
    assert_eq!(projects[1].title, "Iris");
    // No request at all for an empty list.
    assert!(client.projects(&[]).await.unwrap().is_empty());
}

#[tokio::test]
async fn project_versions_sends_loader_and_game_version_filters_as_json_arrays() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/project/sodium/version"))
        .and(query_param("loaders", r#"["fabric"]"#))
        .and(query_param("game_versions", r#"["1.21.1"]"#))
        .respond_with(ResponseTemplate::new(200).set_body_json(vec![sample_version_json("v1")]))
        .expect(1)
        .mount(&server)
        .await;

    let client = client_for(&server).await;
    let filter = VersionsFilter {
        loaders: Some(vec!["fabric".to_string()]),
        game_versions: Some(vec!["1.21.1".to_string()]),
        featured: None,
    };

    let versions = client
        .project_versions("sodium", &filter)
        .await
        .expect("version lookup succeeds");

    assert_eq!(versions.len(), 1);
    assert_eq!(versions[0].id, "v1");
    assert_eq!(versions[0].loaders, vec!["fabric"]);
}

#[tokio::test]
async fn version_files_bulk_hash_lookup() {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/version_files"))
        .and(body_json(serde_json::json!({
            "hashes": ["abc123"],
            "algorithm": "sha1",
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "abc123": sample_version_json("v1"),
        })))
        .expect(1)
        .mount(&server)
        .await;

    let client = client_for(&server).await;
    let result = client
        .version_files(&["abc123".to_string()], HashAlgorithm::Sha1)
        .await
        .expect("bulk hash lookup succeeds");

    assert_eq!(result.len(), 1);
    assert_eq!(result["abc123"].id, "v1");
}

#[tokio::test]
async fn update_version_files_bulk_update_check() {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/version_files/update"))
        .and(body_json(serde_json::json!({
            "hashes": ["abc123"],
            "algorithm": "sha1",
            "loaders": ["fabric"],
            "game_versions": ["1.21.1"],
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "abc123": sample_version_json("v2"),
        })))
        .expect(1)
        .mount(&server)
        .await;

    let client = client_for(&server).await;
    let request = UpdateVersionFilesRequest::new(
        vec!["abc123".to_string()],
        HashAlgorithm::Sha1,
        vec!["fabric".to_string()],
        vec!["1.21.1".to_string()],
    );

    let result = client
        .update_version_files(&request)
        .await
        .expect("update check succeeds");

    assert_eq!(result["abc123"].id, "v2");
}

/// A 5xx on the first attempt should be retried and the second attempt's
/// success returned, rather than the error surfacing to the caller.
#[tokio::test]
async fn retries_on_5xx_then_succeeds() {
    let server = MockServer::start().await;

    let responder = Sequence::new(vec![
        ResponseTemplate::new(503),
        ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "id": "AABBCCDD",
            "team": "team1",
            "title": "Sodium",
            "description": "desc",
            "body": "body",
            "status": "approved",
            "project_type": "mod",
            "published": "2020-01-01T00:00:00Z",
            "updated": "2024-01-01T00:00:00Z",
            "downloads": 1,
            "followers": 1,
        })),
    ]);

    Mock::given(method("GET"))
        .and(path("/project/sodium"))
        .respond_with(responder)
        .expect(2)
        .mount(&server)
        .await;

    let client = client_for(&server).await;
    let project = client.project("sodium").await.expect("retry recovers");
    assert_eq!(project.id, "AABBCCDD");
}

/// A 429 should be retried after honoring `Retry-After`, and the
/// subsequent success returned.
#[tokio::test]
async fn retries_on_429_after_retry_after_then_succeeds() {
    let server = MockServer::start().await;

    let responder = Sequence::new(vec![
        ResponseTemplate::new(429).insert_header("Retry-After", "1"),
        ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "id": "AABBCCDD",
            "team": "team1",
            "title": "Sodium",
            "description": "desc",
            "body": "body",
            "status": "approved",
            "project_type": "mod",
            "published": "2020-01-01T00:00:00Z",
            "updated": "2024-01-01T00:00:00Z",
            "downloads": 1,
            "followers": 1,
        })),
    ]);

    Mock::given(method("GET"))
        .and(path("/project/sodium"))
        .respond_with(responder)
        .expect(2)
        .mount(&server)
        .await;

    let client = client_for(&server).await;

    let started = std::time::Instant::now();
    let project = client.project("sodium").await.expect("retry recovers");
    let elapsed = started.elapsed();

    assert_eq!(project.id, "AABBCCDD");
    // Must have actually waited out the Retry-After window, not retried
    // immediately.
    assert!(
        elapsed >= std::time::Duration::from_millis(900),
        "expected the client to wait ~1s for Retry-After, only waited {elapsed:?}"
    );
}

/// A non-retryable 4xx (anything but 429) should fail immediately, without
/// burning through retry attempts.
#[tokio::test]
async fn non_retryable_4xx_fails_immediately() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/project/does-not-exist"))
        .respond_with(ResponseTemplate::new(404))
        .expect(1)
        .mount(&server)
        .await;

    let client = client_for(&server).await;
    let err = client
        .project("does-not-exist")
        .await
        .expect_err("404 is not retried");

    match err {
        bananium_modrinth::Error::Status { status, .. } => assert_eq!(status, 404),
        other => panic!("expected Error::Status, got {other:?}"),
    }
}

/// The rate limiter must preemptively wait out the reset window once a
/// response reports zero remaining, *before* sending the next request —
/// not just react to a subsequent 429. Uses a 1s synthetic reset window,
/// not the real 60s one.
#[tokio::test]
async fn preemptively_waits_out_an_exhausted_rate_limit_window() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/project/sodium"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("X-Ratelimit-Limit", "300")
                .insert_header("X-Ratelimit-Remaining", "0")
                .insert_header("X-Ratelimit-Reset", "1")
                .set_body_json(serde_json::json!({
                    "id": "AABBCCDD",
                    "team": "team1",
                    "title": "Sodium",
                    "description": "desc",
                    "body": "body",
                    "status": "approved",
                    "project_type": "mod",
                    "published": "2020-01-01T00:00:00Z",
                    "updated": "2024-01-01T00:00:00Z",
                    "downloads": 1,
                    "followers": 1,
                })),
        )
        .mount(&server)
        .await;

    let client = client_for(&server).await;

    // First call: goes out immediately (no prior state), but its response
    // reports the window as exhausted with a 1s reset.
    client.project("sodium").await.expect("first call succeeds");

    // Second call must wait ~1s before the request is even sent, since the
    // client now knows remaining == 0.
    let started = std::time::Instant::now();
    client
        .project("sodium")
        .await
        .expect("second call succeeds");
    let elapsed = started.elapsed();

    assert!(
        elapsed >= std::time::Duration::from_millis(900),
        "expected the client to preemptively wait ~1s before the second request, only waited {elapsed:?}"
    );
}

/// A server that never answers in time must end in `Error::Timeout` after
/// every attempt, with each retry reported to the observer — this is what
/// turns an endless spinner into "Modrinth didn't respond".
#[tokio::test]
async fn a_stalled_server_times_out_and_reports_each_retry() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/project/slow"))
        .respond_with(ResponseTemplate::new(200).set_delay(std::time::Duration::from_secs(5)))
        .mount(&server)
        .await;

    let seen = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let observer_seen = seen.clone();
    let client = ModrinthClient::with_timeouts(
        TEST_USER_AGENT,
        server.uri(),
        std::time::Duration::from_secs(1),
        std::time::Duration::from_millis(200),
    )
    .expect("client builds")
    .with_retry_observer(std::sync::Arc::new(move |n: &RetryNotice| {
        observer_seen
            .lock()
            .unwrap()
            .push((n.endpoint.clone(), n.attempt, n.reason));
    }));

    let err = client.project("slow").await.expect_err("never answers");
    assert!(
        matches!(err, bananium_modrinth::Error::Timeout { .. }),
        "expected Error::Timeout, got {err:?}"
    );
    assert_eq!(
        *seen.lock().unwrap(),
        vec![
            ("project".to_string(), 2, RetryReason::Timeout),
            ("project".to_string(), 3, RetryReason::Timeout),
        ]
    );
}

/// A 5xx retry is reported with its status, and the request still
/// succeeds once the server recovers.
#[tokio::test]
async fn a_server_error_retry_is_reported_with_its_status() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/version_files"))
        .respond_with(Sequence::new(vec![
            ResponseTemplate::new(503),
            ResponseTemplate::new(200).set_body_json(serde_json::json!({})),
        ]))
        .mount(&server)
        .await;

    let seen = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let observer_seen = seen.clone();
    let client = client_for(&server)
        .await
        .with_retry_observer(std::sync::Arc::new(move |n: &RetryNotice| {
            observer_seen.lock().unwrap().push(n.reason);
        }));
    client
        .version_files(&["abc".to_string()], HashAlgorithm::Sha1)
        .await
        .expect("succeeds on retry");
    assert_eq!(*seen.lock().unwrap(), vec![RetryReason::ServerError(503)]);
}

/// When Modrinth's firewall refuses POSTs (a 403, whatever the request),
/// the bulk hash lookup falls back to one GET per hash — and remembers, so
/// the next lookup doesn't try the POST first.
#[tokio::test]
async fn a_blocked_bulk_lookup_falls_back_to_per_file_gets() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/version_files"))
        .respond_with(ResponseTemplate::new(403))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/version_file/known"))
        .and(query_param("algorithm", "sha1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(sample_version_json("v1")))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/version_file/unknown"))
        .respond_with(ResponseTemplate::new(404))
        .mount(&server)
        .await;

    let client = client_for(&server).await;
    let hashes = vec!["known".to_string(), "unknown".to_string()];
    for _ in 0..2 {
        let found = client
            .version_files(&hashes, HashAlgorithm::Sha1)
            .await
            .expect("falls back to GETs");
        assert_eq!(found.len(), 1);
        assert_eq!(found["known"].id, "v1");
    }
}
