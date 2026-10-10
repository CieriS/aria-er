use thiserror::Error;

/// Failure while fetching or parsing data from an upstream source.
#[derive(Debug, Error)]
pub enum SourceError {
    /// The request never produced a usable response, even after retries.
    #[error("request to {url} failed after {attempts} attempt(s): {message}")]
    Transport {
        url: String,
        attempts: u32,
        message: String,
    },
    /// A local file of the source could not be read.
    #[error("cannot read {path}: {message}")]
    Io { path: String, message: String },
    /// The response arrived but does not match the documented format.
    #[error("unexpected format in {context}: {message}")]
    Format { context: String, message: String },
}

/// Failure while reading or writing the raw storage layer.
#[derive(Debug, Error)]
pub enum SinkError {
    #[error("I/O error on {path}: {source}")]
    Io {
        path: String,
        #[source]
        source: std::io::Error,
    },
    /// An object store rejected a read or a write.
    #[error("storage error on {path}: {message}")]
    Storage { path: String, message: String },
    #[error("Parquet error on {path}: {message}")]
    Parquet { path: String, message: String },
    /// An existing file does not have the schema this sink writes.
    #[error("unexpected schema in {path}: {message}")]
    Schema { path: String, message: String },
}
