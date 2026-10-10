//! Where the Parquet files live: the local filesystem, or an object store.

use std::fs;
use std::path::Path;

use aq_core::SinkError;

/// A place files can be read from and written to as a whole.
pub trait Storage: Send + Sync {
    /// Content of the file at `path`; `None` when it does not exist.
    fn read(&self, path: &Path) -> Result<Option<Vec<u8>>, SinkError>;

    /// Stores `data` at `path`. A reader sees the previous content or the new one,
    /// never a partial file.
    fn write(&self, path: &Path, data: Vec<u8>) -> Result<(), SinkError>;
}

fn io_error(path: &Path, source: std::io::Error) -> SinkError {
    SinkError::Io {
        path: path.display().to_string(),
        source,
    }
}

/// The local filesystem; paths are used as they are.
pub struct LocalStorage;

impl Storage for LocalStorage {
    fn read(&self, path: &Path) -> Result<Option<Vec<u8>>, SinkError> {
        match fs::read(path) {
            Ok(data) => Ok(Some(data)),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(io_error(path, error)),
        }
    }

    /// Writes to a temporary file and renames it over the target.
    fn write(&self, path: &Path, data: Vec<u8>) -> Result<(), SinkError> {
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir).map_err(|e| io_error(dir, e))?;
        }
        let mut tmp = path.as_os_str().to_owned();
        tmp.push(".tmp");
        let tmp = Path::new(&tmp);
        fs::write(tmp, data).map_err(|e| io_error(tmp, e))?;
        fs::rename(tmp, path).map_err(|e| io_error(path, e))
    }
}

#[cfg(feature = "gcs")]
pub mod object {
    //! Object stores through the `object_store` crate (compiled with the `gcs` feature).

    use std::path::Path;
    use std::sync::Arc;

    use aq_core::SinkError;
    use object_store::gcp::GoogleCloudStorageBuilder;
    use object_store::path::Path as ObjectPath;
    use object_store::{ObjectStore, ObjectStoreExt, PutPayload};
    use tokio::runtime::{Builder, Runtime};

    use super::Storage;

    /// Any `object_store` backend, driven synchronously.
    pub struct ObjectStorage {
        store: Arc<dyn ObjectStore>,
        runtime: Runtime,
    }

    /// Google Cloud Storage.
    pub type GcsStorage = ObjectStorage;

    fn storage_error(path: &Path, error: impl std::fmt::Display) -> SinkError {
        SinkError::Storage {
            path: path.display().to_string(),
            message: error.to_string(),
        }
    }

    /// Object key of a relative path: its components joined by `/`.
    fn key(path: &Path) -> Result<ObjectPath, SinkError> {
        let text = path
            .to_str()
            .ok_or_else(|| storage_error(path, "path is not valid UTF-8"))?;
        ObjectPath::parse(text.trim_start_matches("./")).map_err(|e| storage_error(path, e))
    }

    impl ObjectStorage {
        pub fn new(store: Arc<dyn ObjectStore>) -> Result<Self, SinkError> {
            let runtime = Builder::new_current_thread()
                .enable_all()
                .build()
                .map_err(|e| storage_error(Path::new(""), e))?;
            Ok(Self { store, runtime })
        }

        /// A bucket of Google Cloud Storage, with credentials taken from the environment
        /// (`GOOGLE_APPLICATION_CREDENTIALS`, or the application default credentials).
        pub fn gcs(bucket: &str) -> Result<Self, SinkError> {
            let store = GoogleCloudStorageBuilder::from_env()
                .with_bucket_name(bucket)
                .build()
                .map_err(|e| storage_error(Path::new(bucket), e))?;
            Self::new(Arc::new(store))
        }
    }

    impl Storage for ObjectStorage {
        fn read(&self, path: &Path) -> Result<Option<Vec<u8>>, SinkError> {
            let key = key(path)?;
            self.runtime.block_on(async {
                match self.store.get(&key).await {
                    Ok(result) => result
                        .bytes()
                        .await
                        .map(|bytes| Some(bytes.to_vec()))
                        .map_err(|e| storage_error(path, e)),
                    Err(object_store::Error::NotFound { .. }) => Ok(None),
                    Err(error) => Err(storage_error(path, error)),
                }
            })
        }

        /// An object is replaced as a whole: the upload is atomic for readers.
        fn write(&self, path: &Path, data: Vec<u8>) -> Result<(), SinkError> {
            let key = key(path)?;
            self.runtime
                .block_on(self.store.put(&key, PutPayload::from(data)))
                .map(|_| ())
                .map_err(|e| storage_error(path, e))
        }
    }
}
