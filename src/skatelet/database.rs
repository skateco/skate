mod peer;
pub(crate) mod resource;

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("{0}")]
    SqlxError(#[from] sqlx::Error),
}

pub type Result<T> = std::result::Result<T, Error>;
