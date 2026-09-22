use tracing_subscriber::EnvFilter;

/// Initialize the global `tracing` subscriber for a binary frontend. Reads
/// `BANANIUM_LOG` (falling back to `info`) and writes to stderr so stdout
/// stays clean for `--format json` and `--dry-run` output.
pub fn init() {
    let filter = EnvFilter::try_from_env("BANANIUM_LOG").unwrap_or_else(|_| EnvFilter::new("info"));
    let _ = tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_target(false)
        .with_writer(std::io::stderr)
        .try_init();
}
