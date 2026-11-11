use crate::{ProfileId, ProfileMetadata, Store, app, files, process};
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use std::fmt;
use std::fs::{self, File, OpenOptions};
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
use std::path::{Path, PathBuf};

const BACKUP_VERSION: u32 = 1;
pub(crate) const MAX_BACKUP_METADATA_BYTES: u64 = 16 * 1024 * 1024;
const TRANSIENT_ROOT_FILES: &[&str] = &[
    "SingletonLock",
    "SingletonCookie",
    "SingletonSocket",
    "RunningChromeVersion",
    "DevToolsActivePort",
];

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(try_from = "String", into = "String")]
pub struct BackupId(String);

impl TryFrom<String> for BackupId {
    type Error = anyhow::Error;

    fn try_from(value: String) -> Result<Self> {
        crate::validate_id(&value, "b-")?;
        Ok(Self(value))
    }
}

impl From<BackupId> for String {
    fn from(value: BackupId) -> Self {
        value.0
    }
}

impl fmt::Display for BackupId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

#[derive(Debug, Serialize)]
pub struct ImportedProfile {
    pub profile: ProfileMetadata,
    pub backup: BackupId,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum Entry {
    Directory { path: PathBuf },
    File { path: PathBuf, sha256: String },
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Snapshot {
    schema_version: u32,
    profile: ProfileMetadata,
    entries: Vec<Entry>,
}

impl Store {
    pub fn import_preview(&self, label: &str, app_path: &Path) -> Result<ImportedProfile> {
        crate::validate_label(label)?;
        let _installation = self.installation_lock(true)?;
        self.check_update_transaction()?;
        let (_, browser) = app::identity(app_path)?;
        let _registry = self.registry_lock(true)?;
        let registry = self.registry_for_creation()?;
        let source = self.root.with_file_name("Stainless");
        files::existing_directory(&source)?;
        process::check_singleton(&source)?;
        ensure!(
            process::last_version(&source)?.as_deref() == Some(&browser.chromium_version),
            "Preview migration requires the matching Stainless app and its exact Chromium Last Version"
        );
        let (mut profile, directory) = self.allocate_profile(label)?;
        profile.last_browser = Some(browser);
        let result = (|| {
            let backup = self.snapshot(&source, &profile)?;
            let snapshot_data = self.backup_directory(&backup)?.join("data");
            copy_tree(&snapshot_data, &directory.join("data"), false)?;
            let snapshot = self.read_snapshot(&backup)?;
            ensure!(
                inventory(&directory.join("data"), false)? == snapshot.entries,
                "Imported profile does not match its backup"
            );
            let profile = self.register_profile(registry, profile, &directory)?;
            Ok(ImportedProfile { profile, backup })
        })();
        result.with_context(|| {
            format!(
                "Preview was not changed; unfinished migration state is retained at {}",
                directory.display()
            )
        })
    }

    pub fn backup(&self, id: &ProfileId) -> Result<BackupId> {
        let _installation = self.installation_lock(true)?;
        self.check_update_transaction()?;
        let _registry = self.registry_lock(false)?;
        let registry = self.registry()?;
        let profile = registry
            .profiles
            .iter()
            .find(|profile| profile.id == *id)
            .context("Unknown profile")?;
        let _owner = self.profile_lock(id)?;
        let data = self.data_for_backup(profile)?;
        self.snapshot(&data, profile)
    }

    pub(crate) fn data_for_backup(&self, profile: &ProfileMetadata) -> Result<PathBuf> {
        let data = self.profile_directory(&profile.id)?.join("data");
        process::check_singleton(&data)?;
        match (&profile.last_browser, process::last_version(&data)?) {
            (Some(browser), Some(version)) => ensure!(
                version == browser.chromium_version,
                "Profile browser identity changed outside its owner; backup pairing needs explicit review"
            ),
            (None, None) => ensure!(
                fs::read_dir(&data)?.next().is_none(),
                "Nonempty profile has no known browser; refusing an unpaired backup"
            ),
            _ => anyhow::bail!("Profile version is missing or unpaired; leave its data untouched"),
        }
        Ok(data)
    }

    pub fn restore(&self, backup: &BackupId, label: &str) -> Result<ProfileMetadata> {
        crate::validate_label(label)?;
        let _installation = self.installation_lock(true)?;
        self.check_update_transaction()?;
        let _registry = self.registry_lock(true)?;
        let registry = self.registry_for_creation()?;
        let snapshot = self.read_snapshot(backup)?;
        let source = self.backup_directory(backup)?.join("data");
        let (mut profile, directory) = self.allocate_profile(label)?;
        profile.last_browser = snapshot.profile.last_browser;
        let result = (|| {
            copy_tree(&source, &directory.join("data"), false)?;
            ensure!(
                inventory(&directory.join("data"), false)? == snapshot.entries,
                "Restored profile does not match its backup"
            );
            self.register_profile(registry, profile, &directory)
        })();
        result.with_context(|| {
            format!(
                "Existing profiles and backup were not changed; unfinished restore is retained at {}",
                directory.display()
            )
        })
    }

    fn backup_directory(&self, id: &BackupId) -> Result<PathBuf> {
        crate::validate_id(&id.0, "b-")?;
        files::existing_directory(&self.root.join("backups"))?;
        let directory = self.root.join("backups").join(&id.0);
        files::existing_directory(&directory)?;
        Ok(directory)
    }

    fn snapshot(&self, source: &Path, profile: &ProfileMetadata) -> Result<BackupId> {
        files::existing_directory(&self.root.join("backups"))?;
        let directory = tempfile::Builder::new()
            .prefix("b-")
            .rand_bytes(16)
            .tempdir_in(self.root.join("backups"))?
            .keep();
        let id = BackupId::try_from(
            directory
                .file_name()
                .and_then(|name| name.to_str())
                .context("Invalid allocated backup identifier")?
                .to_owned(),
        )?;
        let result = (|| -> Result<BackupId> {
            let data = directory.join("data");
            let entries = verified_copy(source, &data)?;
            let snapshot = Snapshot {
                schema_version: BACKUP_VERSION,
                profile: profile.clone(),
                entries,
            };
            // This durable manifest is the commit marker; partial copies are never restorable.
            files::write_json_limited(
                &directory.join("snapshot.json"),
                &snapshot,
                MAX_BACKUP_METADATA_BYTES,
            )?;
            File::open(self.root.join("backups"))?.sync_all()?;
            Ok(id)
        })();
        result.with_context(|| {
            format!(
                "Incomplete backup retained at {}; source data was not changed",
                directory.display()
            )
        })
    }

    fn read_snapshot(&self, id: &BackupId) -> Result<Snapshot> {
        let directory = self.backup_directory(id)?;
        let snapshot: Snapshot =
            files::read_json(&directory.join("snapshot.json"), MAX_BACKUP_METADATA_BYTES)?;
        ensure!(
            snapshot.schema_version == BACKUP_VERSION,
            "Unsupported backup format; no restore was attempted"
        );
        snapshot.profile.validate()?;
        ensure!(
            inventory(&directory.join("data"), false)? == snapshot.entries,
            "Backup is incomplete, corrupted or changed; no restore was attempted"
        );
        Ok(snapshot)
    }

    pub(crate) fn check_update_transaction(&self) -> Result<()> {
        let path = self
            .root
            .with_file_name("Stainless Updater")
            .join("transaction.json");
        match fs::symlink_metadata(&path) {
            Ok(_) => {
                let journal: serde_json::Value =
                    files::read_json(&path, crate::MAX_REGISTRY_BYTES)?;
                ensure!(
                    matches!(journal["phase"].as_str(), Some("healthy" | "rolled_back")),
                    "Finish or recover the pending preview update before managed profile operations"
                );
                Ok(())
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(error).context("Cannot inspect the preview update transaction"),
        }
    }
}

fn children(path: &Path, omit_transient: bool) -> Result<Vec<fs::DirEntry>> {
    files::existing_directory(path)?;
    let mut entries = fs::read_dir(path)?.collect::<std::io::Result<Vec<_>>>()?;
    if omit_transient {
        entries.retain(|entry| {
            !TRANSIENT_ROOT_FILES
                .iter()
                .any(|name| entry.file_name() == *name)
        });
    }
    entries.sort_by_key(fs::DirEntry::file_name);
    Ok(entries)
}

pub(crate) fn copy_tree(source: &Path, destination: &Path, omit_transient: bool) -> Result<()> {
    fs::DirBuilder::new()
        .mode(0o700)
        .create(destination)
        .with_context(|| format!("Cannot create fresh copy {}", destination.display()))?;
    for entry in children(source, omit_transient)? {
        let output = destination.join(entry.file_name());
        let kind = entry.file_type()?;
        if kind.is_dir() {
            copy_tree(&entry.path(), &output, false)?;
        } else {
            ensure!(
                kind.is_file(),
                "Profile contains a symlink or special file: {}",
                entry.path().display()
            );
            let mut input = files::open_file(&entry.path())?;
            let mut output = OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o600)
                .custom_flags(libc::O_NOFOLLOW)
                .open(output)?;
            std::io::copy(&mut input, &mut output)?;
            output.sync_all()?;
        }
    }
    File::open(destination)?.sync_all()?;
    Ok(())
}

pub(crate) fn verified_copy(source: &Path, destination: &Path) -> Result<Vec<Entry>> {
    let entries = inventory(source, true)?;
    copy_tree(source, destination, true)?;
    ensure!(
        inventory(destination, false)? == entries && inventory(source, true)? == entries,
        "Profile changed during backup or its copy is incomplete; no migration was committed"
    );
    Ok(entries)
}

pub(crate) fn inventory(root: &Path, omit_transient: bool) -> Result<Vec<Entry>> {
    fn visit(
        path: &Path,
        root: &Path,
        omit_transient: bool,
        result: &mut Vec<Entry>,
    ) -> Result<()> {
        for entry in children(path, omit_transient)? {
            let relative = entry.path().strip_prefix(root)?.to_owned();
            let kind = entry.file_type()?;
            if kind.is_dir() {
                result.push(Entry::Directory { path: relative });
                visit(&entry.path(), root, false, result)?;
            } else {
                ensure!(
                    kind.is_file(),
                    "Profile contains a symlink or special file: {}",
                    entry.path().display()
                );
                result.push(Entry::File {
                    path: relative,
                    sha256: stainless_release::sha256_reader(files::open_file(&entry.path())?)?,
                });
            }
        }
        Ok(())
    }
    let mut result = Vec::new();
    visit(root, root, omit_transient, &mut result)?;
    Ok(result)
}

#[cfg(test)]
mod tests;
