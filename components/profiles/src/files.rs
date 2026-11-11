use anyhow::{Context, Result, ensure};
use serde::{Serialize, de::DeserializeOwned};
use std::ffi::CString;
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt, PermissionsExt};
use std::path::Path;

pub(crate) fn directory(path: &Path, private: bool) -> Result<()> {
    match fs::symlink_metadata(path) {
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            match fs::DirBuilder::new().mode(0o700).create(path) {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                    existing_directory(path)?;
                }
                Err(error) => return Err(error).context("Cannot create profile directory"),
            }
            File::open(path.parent().context("Missing directory parent")?)?.sync_all()?;
        }
        Err(error) => return Err(error).context("Cannot inspect profile directory"),
    }
    existing_directory(path)?;
    if private {
        fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
    }
    Ok(())
}

pub(crate) fn existing_directory(path: &Path) -> Result<()> {
    let metadata = fs::symlink_metadata(path)
        .with_context(|| format!("Cannot inspect profile directory {}", path.display()))?;
    ensure!(
        metadata.is_dir() && metadata.uid() == unsafe { libc::geteuid() },
        "Profile directory must be a real directory owned by this user: {}",
        path.display()
    );
    Ok(())
}

pub fn open_lock(path: &Path) -> Result<File> {
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(path)
        .with_context(|| format!("Cannot open lock {}", path.display()))?;
    check_file(&file).with_context(|| format!("Unsafe lock file {}", path.display()))?;
    file.set_permissions(fs::Permissions::from_mode(0o600))?;
    Ok(file)
}

pub(crate) fn open_file(path: &Path) -> Result<File> {
    let file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(path)
        .with_context(|| format!("Cannot read profile file {}", path.display()))?;
    check_file(&file).with_context(|| format!("Unsafe profile file {}", path.display()))?;
    Ok(file)
}

fn check_file(file: &File) -> Result<()> {
    let metadata = file.metadata()?;
    ensure!(
        metadata.is_file() && metadata.nlink() == 1 && metadata.uid() == unsafe { libc::geteuid() },
        "Profile metadata/lock must be a regular, unshared file owned by this user"
    );
    Ok(())
}

pub(crate) fn read_json<T: DeserializeOwned>(path: &Path, limit: u64) -> Result<T> {
    let mut bytes = Vec::new();
    open_file(path)?.take(limit + 1).read_to_end(&mut bytes)?;
    ensure!(bytes.len() as u64 <= limit, "Profile metadata is too large");
    serde_json::from_slice(&bytes).context("Invalid profile metadata")
}

pub fn write_json(path: &Path, value: &impl Serialize) -> Result<()> {
    write_json_limited(path, value, u64::MAX)
}

pub(crate) fn write_json_limited(path: &Path, value: &impl Serialize, limit: u64) -> Result<()> {
    let mut bytes = serde_json::to_vec_pretty(value)?;
    bytes.push(b'\n');
    ensure!(
        bytes.len() as u64 <= limit,
        "Profile metadata is too large to persist safely"
    );
    let parent = path.parent().context("Missing JSON parent directory")?;
    let mut temporary = tempfile::NamedTempFile::new_in(parent)?;
    temporary
        .as_file()
        .set_permissions(fs::Permissions::from_mode(0o600))?;
    temporary.as_file_mut().write_all(&bytes)?;
    temporary.as_file().sync_all()?;
    temporary.persist(path).map_err(|error| error.error)?;
    File::open(parent)?.sync_all()?;
    Ok(())
}

pub fn swap(left: &Path, right: &Path) -> Result<()> {
    let left = CString::new(left.as_os_str().as_bytes())?;
    let right = CString::new(right.as_os_str().as_bytes())?;
    #[cfg(target_os = "macos")]
    {
        let result = unsafe { libc::renamex_np(left.as_ptr(), right.as_ptr(), libc::RENAME_SWAP) };
        ensure!(
            result == 0,
            "Atomic application/profile exchange failed: {}",
            std::io::Error::last_os_error()
        );
        Ok(())
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (left, right);
        anyhow::bail!("Atomic update installation currently requires macOS")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exact_json_byte_limit_includes_pretty_formatting_and_newline_without_replacing_on_failure() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("registry.json");
        let value = serde_json::json!({"schema_version": 1, "profiles": []});
        let limit = serde_json::to_vec_pretty(&value).unwrap().len() as u64 + 1;
        write_json_limited(&path, &value, limit).unwrap();
        assert_eq!(fs::metadata(&path).unwrap().len(), limit);
        assert_eq!(read_json::<serde_json::Value>(&path, limit).unwrap(), value);
        let original = fs::read(&path).unwrap();
        assert!(write_json_limited(&path, &value, limit - 1).is_err());
        assert_eq!(fs::read(path).unwrap(), original);
    }
}
