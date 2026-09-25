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

    #[error("Fabric has no loader for Minecraft {0}")]
    NoFabricLoader(String),

    #[error("library {0:?} has no usable maven coordinate or checksum")]
    BadMavenCoordinate(String),

    #[error("malformed checksum file at {0}")]
    BadChecksum(String),
}

pub type Result<T> = std::result::Result<T, Error>;
