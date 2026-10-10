use thiserror::Error;

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, Error)]
pub enum Error {
    /// The API answered with an error body (`{"error": {"code", "message"}}`).
    #[error("{message}")]
    Api {
        status: u16,
        code: String,
        message: String,
    },
    #[error("cannot reach the API: {0}")]
    Network(#[from] reqwest::Error),
    #[error("unexpected API response: {0}")]
    Decode(String),
    #[error("{0}")]
    Auth(String),
    #[error("{0}")]
    Invalid(String),
}

impl Error {
    pub fn status(&self) -> Option<u16> {
        match self {
            Error::Api { status, .. } => Some(*status),
            _ => None,
        }
    }

    pub fn code(&self) -> Option<&str> {
        match self {
            Error::Api { code, .. } => Some(code),
            _ => None,
        }
    }

    pub fn is_not_found(&self) -> bool {
        self.status() == Some(404)
    }
}
