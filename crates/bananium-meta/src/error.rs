#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error(transparent)]
    Net(#[from] bananium_net::Error),

    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    #[error("failed to parse {what} from {url}: {source}")]
    Parse {
        what: &'static str,
        url: String,
        #[source]
        source: serde_json::Error,
    },

    #[error("version {0:?} was not found in the Mojang version manifest")]
    UnknownVersion(String),
}

pub type Result<T> = std::result::Result<T, Error>;
