/// Each large sub-crate error is boxed so `Result<T, Error>` stays small
/// (clippy's `result_large_err`); the `From` impls below box on the way in,
/// so `?` at call sites is unaffected.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error(transparent)]
    Core(Box<bananium_core::Error>),
    #[error(transparent)]
    Net(Box<bananium_net::Error>),
    #[error(transparent)]
    Meta(Box<bananium_meta::Error>),
    #[error(transparent)]
    Store(Box<bananium_store::Error>),
    #[error(transparent)]
    Instance(Box<bananium_instance::Error>),
    #[error(transparent)]
    Launch(Box<bananium_launch::Error>),
    #[error(transparent)]
    Modrinth(Box<bananium_modrinth::Error>),
    #[error(transparent)]
    Java(#[from] bananium_java::Error),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("{0} of {1} downloads failed; first error: {2}")]
    DownloadsFailed(usize, usize, String),
    #[error("instance {0:?} is already running; only one instance of it can run at a time")]
    InstanceAlreadyRunning(String),
    #[error("instance {0:?} wasn't launched from this window, so it can't be stopped from here")]
    NotLaunchedHere(String),
    #[error("no log file named {0:?}")]
    LogNotFound(String),
    #[error(
        "this Fabric instance has no loader version recorded; reinstall it with a loader version"
    )]
    MissingLoaderVersion,
    #[error("instance {0:?} is vanilla; mods and shaders need a Fabric instance")]
    NeedsFabric(String),
    #[error("{project} has no version for Minecraft {mc_version} that this instance can use")]
    NoCompatibleVersion { project: String, mc_version: String },
    #[error("no version with id {0:?}")]
    VersionNotFound(String),
    #[error("Modrinth version {0:?} has no downloadable file")]
    NoFile(String),
    #[error("{0:?} isn't a supported file for this kind of content")]
    UnsupportedFile(String),
    #[error("dependency resolution stopped after {0} projects")]
    TooManyDependencies(usize),
    #[error("project {0:?} isn't installed in this instance")]
    NotInstalled(String),
    #[error(
        "{project} has only beta/alpha versions for Minecraft {mc_version}; pick one explicitly from its version list to install it"
    )]
    OnlyPrereleases { project: String, mc_version: String },
    #[error("can't install: {0}")]
    Incompatible(String),
    #[error("{0:?} isn't a screenshot")]
    NotAScreenshot(String),
    #[error("{0:?} isn't a path inside the instance's game folder")]
    InvalidPath(String),
    #[error("{0:?} is too large to edit here (over 2 MiB); open it in an external editor")]
    FileTooLarge(String),
    #[error("{0:?} isn't a text file")]
    NotText(String),
    #[error("{0:?} already exists")]
    FileExists(String),
    #[error(
        "the chosen Java {0:?} can't be run; pick another in the instance's or global Java setting"
    )]
    JavaNotRunnable(String),
    #[error(
        "no Java runtime for {component}{}: Mojang publishes none for this platform and no matching Java was found; choose one in the instance's Java setting",
        major.map(|m| format!(" (Java {m})")).unwrap_or_default()
    )]
    NoSuitableJava {
        component: String,
        major: Option<u32>,
    },
    #[error("not a valid .mrpack: {0}")]
    BadModpack(String),
    #[error("{0}")]
    UnsupportedModpack(String),
}

/// Generates a `From<$source> for Error` that boxes on the way in. Written
/// as a macro (rather than relying on thiserror's `#[from]`) because
/// `#[from]` requires the enum field's type to match the source type
/// exactly — it can't insert the `Box::new` for us, so each conversion is
/// spelled out here instead of six near-identical hand-written `impl` blocks.
macro_rules! impl_boxed_from {
    ($source:ty, $variant:ident) => {
        impl From<$source> for Error {
            fn from(err: $source) -> Self {
                Error::$variant(Box::new(err))
            }
        }
    };
}

impl_boxed_from!(bananium_core::Error, Core);
impl_boxed_from!(bananium_net::Error, Net);
impl_boxed_from!(bananium_meta::Error, Meta);
impl_boxed_from!(bananium_store::Error, Store);
impl_boxed_from!(bananium_instance::Error, Instance);
impl_boxed_from!(bananium_launch::Error, Launch);
impl_boxed_from!(bananium_modrinth::Error, Modrinth);

pub type Result<T> = std::result::Result<T, Error>;
