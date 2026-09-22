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
    Java(#[from] bananium_java::Error),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("{0} of {1} downloads failed; first error: {2}")]
    DownloadsFailed(usize, usize, String),
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

pub type Result<T> = std::result::Result<T, Error>;
