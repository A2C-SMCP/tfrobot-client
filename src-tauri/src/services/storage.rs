use serde::Serialize;
use std::fs;
use std::io::{self, Write};
use std::path::Path;

pub fn write_json_atomically<T: Serialize + ?Sized>(
    path: &Path,
    data: &T,
) -> Result<(), AtomicJsonWriteError> {
    let content = serde_json::to_vec_pretty(data)?;
    write_atomically(path, &content)?;
    Ok(())
}

pub(crate) fn write_atomically(path: &Path, content: &[u8]) -> io::Result<()> {
    let parent = path.parent().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("path has no parent: {}", path.display()),
        )
    })?;
    fs::create_dir_all(parent)?;

    let mut temporary = tempfile::NamedTempFile::new_in(parent)?;
    temporary.write_all(content)?;
    temporary.as_file().sync_all()?;
    temporary.persist(path).map_err(|error| error.error)?;

    #[cfg(unix)]
    fs::File::open(parent)?.sync_all()?;

    Ok(())
}

#[derive(Debug, thiserror::Error)]
pub enum AtomicJsonWriteError {
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("IO error: {0}")]
    Io(#[from] io::Error),
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::ser::Error as _;

    struct FailingSerialize;

    impl Serialize for FailingSerialize {
        fn serialize<S>(&self, _serializer: S) -> Result<S::Ok, S::Error>
        where
            S: serde::Serializer,
        {
            Err(S::Error::custom("intentional serialization failure"))
        }
    }

    #[test]
    fn atomically_creates_and_replaces_json_without_temp_files() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("nested/config.json");

        write_json_atomically(&path, &serde_json::json!({"version": 1})).unwrap();
        write_json_atomically(&path, &serde_json::json!({"version": 2})).unwrap();

        let value: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        assert_eq!(value, serde_json::json!({"version": 2}));
        assert_eq!(fs::read_dir(path.parent().unwrap()).unwrap().count(), 1);
    }

    #[test]
    fn serialization_failure_preserves_existing_file() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("config.json");
        write_json_atomically(&path, &serde_json::json!({"version": 1})).unwrap();
        let original = fs::read(&path).unwrap();

        assert!(matches!(
            write_json_atomically(&path, &FailingSerialize),
            Err(AtomicJsonWriteError::Json(_))
        ));

        assert_eq!(fs::read(&path).unwrap(), original);
        assert_eq!(fs::read_dir(directory.path()).unwrap().count(), 1);
    }

    #[test]
    fn failed_persist_preserves_existing_target_and_cleans_up_temp_file() {
        let directory = tempfile::tempdir().unwrap();
        let target = directory.path().join("config.json");
        fs::create_dir(&target).unwrap();

        assert!(matches!(
            write_json_atomically(&target, &serde_json::json!({"version": 1})),
            Err(AtomicJsonWriteError::Io(_))
        ));

        assert!(target.is_dir());
        assert_eq!(fs::read_dir(directory.path()).unwrap().count(), 1);
    }
}
