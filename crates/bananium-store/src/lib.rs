//! Content-addressed blob store, hardlink/reflink materialization, and garbage collection.

pub mod error;

use std::path::{Path, PathBuf};

use bananium_core::Paths;

pub use error::{Error, Result};

/// How a blob ended up at its destination. Mostly useful for logging and
/// for the dedup-gate manual test (`stat` should show shared inodes for
/// `Hardlinked`, and `Reflinked` costs no extra disk at all).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Materialization {
    /// `dest` already existed; nothing was done.
    AlreadyPresent,
    /// Copy-on-write clone (e.g. `FICLONE` on btrfs/XFS) — costs no extra
    /// disk space at all, even more than a hardlink.
    Reflinked,
    /// Same inode as the store blob — costs no extra disk space.
    Hardlinked,
    /// Filesystem supported neither reflink nor hardlink (e.g. the
    /// destination is on a different volume) — a real byte-for-byte copy.
    Copied,
}

/// A thin view over `~/.bananium/store/<sha1[0..2]>/<sha1>`. The download
/// engine writes blobs directly at `Paths::store_blob`, already
/// checksum-verified; this crate's job is placing *views* of those blobs
/// (native-extraction caches, per-instance library layouts, the assets
/// tree) without duplicating the underlying bytes.
pub struct BlobStore {
    paths: Paths,
}

impl BlobStore {
    pub fn new(paths: Paths) -> Self {
        Self { paths }
    }

    /// The content-addressed path a blob lives (or would live) at. Doesn't
    /// check existence — see [`BlobStore::contains`] for that.
    pub fn blob_path(&self, sha1_hex: &str) -> PathBuf {
        self.paths.store_blob(sha1_hex)
    }

    /// Whether a blob with this SHA-1 is already in the store.
    pub fn contains(&self, sha1_hex: &str) -> bool {
        self.blob_path(sha1_hex).is_file()
    }

    /// Place a view of `sha1_hex`'s blob at `dest`, preferring (in order) a
    /// copy-on-write reflink, a hardlink, and finally a full byte copy —
    /// whichever the filesystem actually supports. A no-op if `dest`
    /// already exists.
    pub fn materialize(&self, sha1_hex: &str, dest: &Path) -> Result<Materialization> {
        let src = self.blob_path(sha1_hex);
        if !src.is_file() {
            return Err(Error::MissingBlob(sha1_hex.to_string()));
        }
        if let Some(parent) = dest.parent() {
            std::fs::create_dir_all(parent)?;
        }
        if dest.is_file() {
            return Ok(Materialization::AlreadyPresent);
        }

        if reflink_copy::reflink(&src, dest).is_ok() {
            return Ok(Materialization::Reflinked);
        }
        match std::fs::hard_link(&src, dest) {
            Ok(()) => Ok(Materialization::Hardlinked),
            Err(err) => {
                tracing::debug!(
                    ?err,
                    "hardlink into store failed, falling back to a full copy"
                );
                std::fs::copy(&src, dest)?;
                Ok(Materialization::Copied)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_blob(paths: &Paths, contents: &[u8]) -> String {
        use sha1::{Digest, Sha1};
        let mut hasher = Sha1::new();
        hasher.update(contents);
        let hash: String = hasher
            .finalize()
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();
        let path = paths.store_blob(&hash);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, contents).unwrap();
        hash
    }

    #[test]
    fn materialize_shares_the_inode_via_hardlink() {
        let dir = tempfile::tempdir().unwrap();
        let paths = Paths::at(dir.path());
        let hash = write_blob(&paths, b"shared library bytes");

        let store = BlobStore::new(paths.clone());
        let dest = dir.path().join("instance").join("libs").join("lib.jar");
        let result = store.materialize(&hash, &dest).unwrap();
        assert!(matches!(
            result,
            Materialization::Hardlinked | Materialization::Reflinked
        ));
        assert_eq!(std::fs::read(&dest).unwrap(), b"shared library bytes");

        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            let src_meta = std::fs::metadata(store.blob_path(&hash)).unwrap();
            let dest_meta = std::fs::metadata(&dest).unwrap();
            assert_eq!(src_meta.ino(), dest_meta.ino());
            assert!(src_meta.nlink() >= 2);
        }
    }

    #[test]
    fn materialize_is_idempotent() {
        let dir = tempfile::tempdir().unwrap();
        let paths = Paths::at(dir.path());
        let hash = write_blob(&paths, b"idempotent bytes");
        let store = BlobStore::new(paths);
        let dest = dir.path().join("lib.jar");
        assert_ne!(
            store.materialize(&hash, &dest).unwrap(),
            Materialization::AlreadyPresent
        );
        assert_eq!(
            store.materialize(&hash, &dest).unwrap(),
            Materialization::AlreadyPresent
        );
    }

    #[test]
    fn missing_blob_is_an_error() {
        let dir = tempfile::tempdir().unwrap();
        let paths = Paths::at(dir.path());
        let store = BlobStore::new(paths);
        let dest = dir.path().join("lib.jar");
        assert!(matches!(
            store.materialize("deadbeef", &dest),
            Err(Error::MissingBlob(_))
        ));
    }
}
