use std::io::Write as _;
use std::net::TcpListener;
use std::sync::Arc;

use bananium_net::{no_progress, DownloadSpec, Downloader};
use sha1::{Digest, Sha1};

/// A tiny single-threaded HTTP/1.1 server, just capable enough to serve one
/// fixed byte body and honor `Range: bytes=N-`. Good enough to exercise the
/// downloader's resume path without pulling in a whole mock-HTTP crate.
fn spawn_range_server(body: &'static [u8]) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let mut stream = match stream {
                Ok(s) => s,
                Err(_) => continue,
            };
            use std::io::{BufRead, BufReader};
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let mut request_line = String::new();
            reader.read_line(&mut request_line).unwrap();
            let mut range_start = 0usize;
            loop {
                let mut line = String::new();
                if reader.read_line(&mut line).unwrap_or(0) == 0 {
                    break;
                }
                if line.trim().is_empty() {
                    break;
                }
                if let Some(rest) = line.to_ascii_lowercase().strip_prefix("range:") {
                    if let Some(eq) = rest.find('=') {
                        let spec = rest[eq + 1..].trim();
                        if let Some(dash) = spec.find('-') {
                            range_start = spec[..dash].trim().parse().unwrap_or(0);
                        }
                    }
                }
            }

            if range_start >= body.len() {
                // Real servers (including resources.download.minecraft.net)
                // reject a Range request whose start is at or past the
                // actual content length with 416, rather than a 206 with an
                // empty body.
                let header = format!(
                    "HTTP/1.1 416 Range Not Satisfiable\r\nContent-Range: bytes */{}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                    body.len()
                );
                stream.write_all(header.as_bytes()).unwrap();
                continue;
            }

            let slice = &body[range_start..];
            if range_start > 0 {
                let header = format!(
                    "HTTP/1.1 206 Partial Content\r\nContent-Range: bytes {}-{}/{}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    range_start,
                    body.len() - 1,
                    body.len(),
                    slice.len()
                );
                stream.write_all(header.as_bytes()).unwrap();
            } else {
                let header = format!(
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    slice.len()
                );
                stream.write_all(header.as_bytes()).unwrap();
            }
            stream.write_all(slice).unwrap();
        }
    });
    format!("http://{addr}")
}

fn sha1_hex(data: &[u8]) -> String {
    let mut hasher = Sha1::new();
    hasher.update(data);
    hasher
        .finalize()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

#[tokio::test]
async fn downloads_and_verifies_checksum() {
    let body: &'static [u8] = b"bananium is a low-ram minecraft launcher, honest.";
    let base = spawn_range_server(body);

    let dir = tempfile::tempdir().unwrap();
    let dest = dir.path().join("blob");

    let client = reqwest::Client::new();
    let downloader = Downloader::new(client, 4);
    let spec = DownloadSpec {
        url: format!("{base}/file"),
        dest: dest.clone(),
        expected_sha1: Some(sha1_hex(body)),
        expected_size: Some(body.len() as u64),
        task_id: "t1".into(),
        label: "file".into(),
    };

    downloader.download(&spec, no_progress()).await.unwrap();
    let on_disk = std::fs::read(&dest).unwrap();
    assert_eq!(on_disk, body);
}

#[tokio::test]
async fn resumes_a_partial_download() {
    let body: &'static [u8] =
        b"the quick banana jumps over the lazy launcher, repeatedly, to pad this out a little.";
    let base = spawn_range_server(body);

    let dir = tempfile::tempdir().unwrap();
    let dest = dir.path().join("blob");
    // Pre-seed a partial ".part" file, as if a previous attempt was interrupted.
    std::fs::write(dest.with_file_name("blob.part"), &body[..20]).unwrap();

    let client = reqwest::Client::new();
    let downloader = Downloader::new(client, 1);
    let spec = DownloadSpec {
        url: format!("{base}/file"),
        dest: dest.clone(),
        expected_sha1: Some(sha1_hex(body)),
        expected_size: Some(body.len() as u64),
        task_id: "t1".into(),
        label: "file".into(),
    };

    downloader.download(&spec, no_progress()).await.unwrap();
    let on_disk = std::fs::read(&dest).unwrap();
    assert_eq!(on_disk, body);
}

#[tokio::test]
async fn already_correct_file_is_left_alone_offline() {
    let body: &'static [u8] = b"cached bytes should never trigger a network call at all";
    let dir = tempfile::tempdir().unwrap();
    let dest = dir.path().join("blob");
    std::fs::write(&dest, body).unwrap();

    let client = reqwest::Client::new();
    let downloader = Downloader::new(client, 1);
    let spec = DownloadSpec {
        // Deliberately unroutable: if the engine tried to hit the network
        // for a file that already verifies, this would hang/fail instead.
        url: "http://127.0.0.1:1/unreachable".into(),
        dest: dest.clone(),
        expected_sha1: Some(sha1_hex(body)),
        expected_size: Some(body.len() as u64),
        task_id: "t1".into(),
        label: "file".into(),
    };

    let result = tokio::time::timeout(
        std::time::Duration::from_secs(5),
        downloader.download(&spec, no_progress()),
    )
    .await
    .expect("should not need the network for an already-verified file");
    result.unwrap();
}

#[tokio::test]
async fn download_all_reports_one_result_per_spec_in_order() {
    let body: &'static [u8] = b"ok";
    let base = spawn_range_server(body);
    let dir = tempfile::tempdir().unwrap();

    let client = reqwest::Client::new();
    let downloader = Downloader::new(client, 2);

    let specs: Vec<_> = (0..5)
        .map(|i| DownloadSpec {
            url: format!("{base}/file"),
            dest: dir.path().join(format!("blob-{i}")),
            expected_sha1: Some(sha1_hex(body)),
            expected_size: Some(body.len() as u64),
            task_id: format!("t{i}"),
            label: format!("file {i}"),
        })
        .collect();

    let results = downloader.download_all(specs, Arc::new(|_| {})).await;
    assert_eq!(results.len(), 5);
    for r in results {
        r.unwrap();
    }
}

/// Regression test for a real bug: a `.part` file that's already at (or
/// past) the expected size can never be resumed — the server rightfully
/// rejects `Range: bytes=<size>-` with 416 — but nothing used to reset it,
/// so every retry sent the exact same invalid request and failed
/// identically forever, requiring a human to delete the file by hand.
#[tokio::test]
async fn stale_full_size_partial_file_is_discarded_and_redownloaded() {
    let body: &'static [u8] = b"the correct bytes for this asset, published by mojang.";
    let base = spawn_range_server(body);

    let dir = tempfile::tempdir().unwrap();
    let dest = dir.path().join("blob");
    // A `.part` file that's the right *size* but wrong *content* — as if
    // an earlier run wrote something stale or got corrupted mid-write.
    // Without the fix, `existing >= expected_size` on this file means
    // every single attempt would request `Range: bytes={len}-`, the server
    // would 416, and the download would never succeed.
    let garbage = vec![b'x'; body.len()];
    std::fs::write(dest.with_file_name("blob.part"), &garbage).unwrap();

    let client = reqwest::Client::new();
    let downloader = Downloader::new(client, 1);
    let spec = DownloadSpec {
        url: format!("{base}/file"),
        dest: dest.clone(),
        expected_sha1: Some(sha1_hex(body)),
        expected_size: Some(body.len() as u64),
        task_id: "t1".into(),
        label: "file".into(),
    };

    downloader.download(&spec, no_progress()).await.unwrap();
    assert_eq!(std::fs::read(&dest).unwrap(), body);
}

/// A `.part` file that's already a *correct*, complete download (just never
/// renamed — e.g. the process was killed between finishing the write and
/// committing it) should be committed directly, with no network call at
/// all: the download's URL is deliberately unroutable here.
#[tokio::test]
async fn complete_but_unrenamed_partial_file_is_committed_without_network() {
    let body: &'static [u8] = b"already fully downloaded, just never got renamed into place.";
    let dir = tempfile::tempdir().unwrap();
    let dest = dir.path().join("blob");
    std::fs::write(dest.with_file_name("blob.part"), body).unwrap();

    let client = reqwest::Client::new();
    let downloader = Downloader::new(client, 1);
    let spec = DownloadSpec {
        url: "http://127.0.0.1:1/unreachable".into(),
        dest: dest.clone(),
        expected_sha1: Some(sha1_hex(body)),
        expected_size: Some(body.len() as u64),
        task_id: "t1".into(),
        label: "file".into(),
    };

    let result = tokio::time::timeout(
        std::time::Duration::from_secs(5),
        downloader.download(&spec, no_progress()),
    )
    .await
    .expect("should not need the network for an already-complete .part file");
    result.unwrap();
    assert_eq!(std::fs::read(&dest).unwrap(), body);
    assert!(!dest.with_file_name("blob.part").exists());
}

/// Two downloaders fetching the same destination at once (two installs
/// sharing a library) must not both stream into one `.part` file: the
/// second waits for the first and then finds the file already in place, so
/// the server sees exactly one request and both calls succeed.
#[tokio::test]
async fn concurrent_downloads_of_one_destination_fetch_it_once() {
    use std::io::{BufRead, BufReader};
    use std::sync::atomic::{AtomicUsize, Ordering};

    let body: &'static [u8] = b"shared library bytes, fetched exactly once";
    let hits = Arc::new(AtomicUsize::new(0));
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let server_hits = hits.clone();
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { continue };
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            loop {
                let mut line = String::new();
                if reader.read_line(&mut line).unwrap_or(0) == 0 || line.trim().is_empty() {
                    break;
                }
            }
            server_hits.fetch_add(1, Ordering::SeqCst);
            // Slow enough that the second download starts while the first
            // is still in flight.
            std::thread::sleep(std::time::Duration::from_millis(300));
            let header = format!(
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            );
            stream.write_all(header.as_bytes()).unwrap();
            stream.write_all(body).unwrap();
        }
    });

    let dir = tempfile::tempdir().unwrap();
    let spec = DownloadSpec {
        url: format!("http://{addr}/file"),
        dest: dir.path().join("blob"),
        expected_sha1: Some(sha1_hex(body)),
        expected_size: Some(body.len() as u64),
        task_id: "t".into(),
        label: "file".into(),
    };
    let a = Downloader::new(reqwest::Client::new(), 2);
    let b = Downloader::new(reqwest::Client::new(), 2);
    let (ra, rb) = tokio::join!(
        a.download(&spec, no_progress()),
        b.download(&spec, no_progress())
    );
    ra.unwrap();
    rb.unwrap();
    assert_eq!(std::fs::read(&spec.dest).unwrap(), body);
    assert_eq!(hits.load(Ordering::SeqCst), 1);
}
