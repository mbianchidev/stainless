use crate::backups::{Entry, MAX_BACKUP_METADATA_BYTES, inventory, verified_copy};
use crate::{BrowserIdentity, ProfileId, ProfileMetadata, Registry, Store, files, process};
use anyhow::{Context, Result, ensure};
use fs2::FileExt;
use serde::{Deserialize, Serialize};
use stainless_release::Product;
use std::fs::{self, File};
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Snapshot {
    schema_version: u32,
    transaction: String,
    old: BrowserIdentity,
    new: BrowserIdentity,
    registry: Option<Registry>,
    preview: Option<Vec<Entry>>,
    managed: Vec<ManagedSnapshot>,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct ManagedSnapshot {
    id: ProfileId,
    entries: Vec<Entry>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum Root {
    Preview,
    Managed { id: ProfileId },
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct DirectoryIdentity {
    device: u64,
    inode: u64,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct RestoreRoot {
    root: Root,
    // Directory identities survive an exchange, including a crash before its journal update.
    original: Option<DirectoryIdentity>,
    restored: Option<DirectoryIdentity>,
    staging: Option<String>,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Recovery {
    schema_version: u32,
    transaction: String,
    failed_registry: Option<Registry>,
    roots: Vec<RestoreRoot>,
}

/// Trusted updater maintenance authority, separate from the metadata request contract.
pub struct UpdateProfiles {
    preview: PathBuf,
    store: Option<Store>,
    registry: Option<Registry>,
    _installation: File,
    _registry_lock: Option<File>,
    _owners: Vec<File>,
}

impl UpdateProfiles {
    pub fn lock(preview: &Path, installation: File) -> Result<Self> {
        installation
            .try_lock_exclusive()
            .context("Quit every Stainless browser before updating or recovering profiles")?;
        ensure!(preview.is_absolute(), "Profile root must be absolute");
        files::existing_directory(preview.parent().context("Missing profile parent")?)?;
        let root = preview.with_file_name("Stainless Profiles");
        let store = match fs::symlink_metadata(&root) {
            Ok(_) => {
                files::existing_directory(&root)?;
                files::existing_directory(&root.join("profiles"))?;
                files::existing_directory(&root.join("backups"))?;
                Some(Store { root })
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
            Err(error) => return Err(error).context("Cannot inspect managed profiles"),
        };
        let registry_lock = store
            .as_ref()
            .map(|store| store.registry_lock(true))
            .transpose()?;
        let registry = store.as_ref().map(Store::registry).transpose()?;
        let mut owners = Vec::new();
        if let (Some(store), Some(registry)) = (&store, &registry) {
            for profile in &registry.profiles {
                owners.push(store.profile_lock(&profile.id)?);
            }
        }
        Ok(Self {
            preview: preview.to_owned(),
            store,
            registry,
            _installation: installation,
            _registry_lock: registry_lock,
            _owners: owners,
        })
    }

    pub fn capture(
        &self,
        destination: &Path,
        transaction: &str,
        old: &BrowserIdentity,
        new: &BrowserIdentity,
    ) -> Result<bool> {
        validate_transaction(transaction)?;
        old.validate()?;
        new.validate()?;
        new.ensure_not_older_than(old)?;
        let preview = if directory_identity(&self.preview)?.is_some() {
            process::check_singleton(&self.preview)?;
            if let Some(version) = process::last_version(&self.preview)? {
                ensure!(
                    crate::chromium_version(&old.chromium_version)?
                        >= crate::chromium_version(&version)?,
                    "Preview data is newer than the installed browser; no safe rollback pairing exists"
                );
            }
            Some(verified_copy(&self.preview, &destination.join("profile"))?)
        } else {
            None
        };
        let mut managed = Vec::new();
        if let (Some(store), Some(registry)) = (&self.store, &self.registry) {
            files::directory(&destination.join("profiles"), true)?;
            for profile in &registry.profiles {
                if let Some(previous) = &profile.last_browser {
                    old.ensure_not_older_than(previous)?;
                    new.ensure_not_older_than(previous)?;
                }
                let source = store.data_for_backup(profile)?;
                let entries = verified_copy(
                    &source,
                    &destination.join("profiles").join(profile.id.to_string()),
                )?;
                managed.push(ManagedSnapshot {
                    id: profile.id.clone(),
                    entries,
                });
            }
            File::open(destination.join("profiles"))?.sync_all()?;
        }
        let had_preview = preview.is_some();
        let snapshot = Snapshot {
            schema_version: 1,
            transaction: transaction.to_owned(),
            old: old.clone(),
            new: new.clone(),
            registry: self.registry.clone(),
            preview,
            managed,
        };
        files::write_json_limited(
            &destination.join("profiles.json"),
            &snapshot,
            MAX_BACKUP_METADATA_BYTES,
        )?;
        Ok(had_preview)
    }

    pub fn adopt_legacy(
        &self,
        destination: &Path,
        transaction: &str,
        old: &BrowserIdentity,
        new: &BrowserIdentity,
        had_preview: bool,
    ) -> Result<()> {
        self.check_legacy_rollback()?;
        let snapshot = Snapshot {
            schema_version: 1,
            transaction: transaction.to_owned(),
            old: old.clone(),
            new: new.clone(),
            registry: self.registry.clone(),
            preview: if had_preview {
                Some(inventory(&destination.join("profile"), true)?)
            } else {
                None
            },
            managed: Vec::new(),
        };
        validate_snapshot(&snapshot, transaction, &old.product, &new.product)?;
        files::write_json_limited(
            &destination.join("profiles.json"),
            &snapshot,
            MAX_BACKUP_METADATA_BYTES,
        )
    }

    pub fn check_legacy_rollback(&self) -> Result<()> {
        ensure!(
            profiles(&self.registry).is_empty(),
            "Legacy update has no managed-profile snapshots; preserve all roots and use explicit recovery"
        );
        Ok(())
    }

    pub fn check_snapshot(
        destination: &Path,
        transaction: &str,
        old: &Product,
        new: &Product,
    ) -> Result<bool> {
        read_snapshot(destination, transaction, old, new).map(|snapshot| snapshot.preview.is_some())
    }

    pub fn prepare_rollback(
        &self,
        destination: &Path,
        transaction: &str,
        old: &Product,
        new: &Product,
    ) -> Result<()> {
        let snapshot = read_snapshot(destination, transaction, old, new)?;
        verify_snapshot(destination, &snapshot)?;
        let recovery_path = destination.join("recovery.json");
        match fs::symlink_metadata(&recovery_path) {
            Ok(_) => {
                self.recovery(destination, &snapshot)?;
                return Ok(());
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error).context("Cannot inspect profile recovery plan"),
        }
        check_registry_change(&snapshot.registry, &self.registry, &snapshot.new)?;
        let mut roots = Vec::new();
        for (root, entries, backup) in snapshot_roots(destination, &snapshot) {
            let live = self.live(&root)?;
            let failed = self.failed(&root, transaction)?;
            ensure!(
                directory_identity(&failed)?.is_none(),
                "Failed-profile destination already exists: {}",
                failed.display()
            );
            let original = directory_identity(&live)?;
            let (restored, staging) = if let Some(entries) = entries {
                let temporary = tempfile::Builder::new()
                    .prefix("r-")
                    .rand_bytes(16)
                    .tempdir_in(live.parent().context("Missing profile parent")?)?
                    .keep();
                let staging = temporary
                    .file_name()
                    .and_then(|name| name.to_str())
                    .context("Invalid profile recovery directory")?
                    .to_owned();
                let copy = temporary.join("data");
                let copied = verified_copy(&backup, &copy).with_context(|| {
                    format!("Recovery preparation retained at {}", temporary.display())
                })?;
                ensure!(
                    copied.as_slice() == entries,
                    "Recovery copy differs from its profile snapshot"
                );
                File::open(&temporary)?.sync_all()?;
                File::open(temporary.parent().context("Missing recovery parent")?)?.sync_all()?;
                (directory_identity(&copy)?, Some(staging))
            } else {
                (None, None)
            };
            roots.push(RestoreRoot {
                root,
                original,
                restored,
                staging,
            });
        }
        let recovery = Recovery {
            schema_version: 1,
            transaction: transaction.to_owned(),
            failed_registry: self.registry.clone(),
            roots,
        };
        files::write_json_limited(&recovery_path, &recovery, MAX_BACKUP_METADATA_BYTES)
    }

    pub fn restore(
        &self,
        destination: &Path,
        transaction: &str,
        old: &Product,
        new: &Product,
    ) -> Result<()> {
        let snapshot = read_snapshot(destination, transaction, old, new)?;
        verify_snapshot(destination, &snapshot)?;
        let recovery = self.recovery(destination, &snapshot)?;
        for (restore, (_, entries, _)) in recovery
            .roots
            .iter()
            .zip(snapshot_roots(destination, &snapshot))
        {
            self.restore_root(restore, transaction, entries).with_context(|| {
                format!("Profile recovery is unfinished for {:?}; no application rollback was committed", restore.root)
            })?;
        }
        if let Some(registry) = &snapshot.registry {
            let store = self
                .store
                .as_ref()
                .context("Managed registry disappeared during recovery")?;
            files::write_json_limited(
                &store.root.join("registry.json"),
                registry,
                crate::MAX_REGISTRY_BYTES,
            )?;
        }
        Ok(())
    }

    fn recovery(&self, destination: &Path, snapshot: &Snapshot) -> Result<Recovery> {
        let recovery: Recovery = files::read_json(
            &destination.join("recovery.json"),
            MAX_BACKUP_METADATA_BYTES,
        )?;
        ensure!(
            recovery.schema_version == 1
                && recovery.transaction == snapshot.transaction
                && recovery
                    .roots
                    .iter()
                    .map(|root| root.root.clone())
                    .eq(
                        std::iter::once(Root::Preview).chain(snapshot.managed.iter().map(
                            |profile| Root::Managed {
                                id: profile.id.clone()
                            }
                        ))
                    ),
            "Unsupported or inconsistent profile recovery plan"
        );
        check_registry_change(&snapshot.registry, &recovery.failed_registry, &snapshot.new)?;
        ensure!(
            profiles(&self.registry) == profiles(&snapshot.registry)
                || profiles(&self.registry) == profiles(&recovery.failed_registry),
            "Profile registry changed during recovery; no further roots were restored"
        );
        for (root, (_, entries, _)) in recovery
            .roots
            .iter()
            .zip(snapshot_roots(destination, snapshot))
        {
            ensure!(
                root.restored.is_some() == root.staging.is_some()
                    && root.restored.is_some() == entries.is_some(),
                "Incomplete profile recovery identity"
            );
            if let Some(staging) = &root.staging {
                crate::validate_id(staging, "r-")?;
            }
        }
        Ok(recovery)
    }

    fn live(&self, root: &Root) -> Result<PathBuf> {
        match root {
            Root::Preview => Ok(self.preview.clone()),
            Root::Managed { id } => Ok(self
                .store
                .as_ref()
                .context("Managed profile store is missing")?
                .profile_directory(id)?
                .join("data")),
        }
    }

    fn failed(&self, root: &Root, transaction: &str) -> Result<PathBuf> {
        let live = self.live(root)?;
        Ok(match root {
            Root::Preview => {
                live.with_file_name(format!(".Stainless.failed-profile-{transaction}"))
            }
            Root::Managed { .. } => live.with_file_name(format!("failed-{transaction}")),
        })
    }

    fn restore_root(
        &self,
        restore: &RestoreRoot,
        transaction: &str,
        entries: Option<&[Entry]>,
    ) -> Result<()> {
        let live = self.live(&restore.root)?;
        let failed = self.failed(&restore.root, transaction)?;
        let parent = live.parent().context("Missing profile parent")?;
        let temporary = restore.staging.as_ref().map(|name| parent.join(name));
        if let Some(temporary) = &temporary
            && directory_identity(temporary)?.is_some()
        {
            files::existing_directory(temporary)?;
        }
        let staged = temporary.as_ref().map(|path| path.join("data"));
        let current = directory_identity(&live)?;
        let failed_identity = directory_identity(&failed)?;
        if let Some(restored) = restore.restored {
            let staged = staged.as_ref().context("Missing recovery staging path")?;
            if current != Some(restored) {
                ensure!(
                    current == restore.original
                        && failed_identity.is_none()
                        && directory_identity(staged)? == Some(restored),
                    "Profile recovery paths changed; retained data was not overwritten"
                );
                ensure!(
                    inventory(staged, false)?.as_slice()
                        == entries.context("Missing profile snapshot inventory")?,
                    "Prepared profile recovery copy changed"
                );
                if current.is_some() {
                    files::swap(&live, staged)?;
                } else {
                    fs::rename(staged, &live)?;
                }
                sync_parents(&live, staged)?;
            }
            ensure!(
                inventory(&live, false)?.as_slice()
                    == entries.context("Missing profile snapshot inventory")?,
                "Restored profile changed during interrupted recovery"
            );
            if let Some(original) = restore.original {
                if failed_identity != Some(original) {
                    ensure!(
                        failed_identity.is_none() && directory_identity(staged)? == Some(original),
                        "Failed profile identity changed"
                    );
                    fs::rename(staged, &failed)?;
                    sync_parents(staged, &failed)?;
                } else {
                    ensure!(
                        directory_identity(staged)?.is_none(),
                        "Unexpected remaining profile recovery copy"
                    );
                }
            } else {
                ensure!(
                    failed_identity.is_none() && directory_identity(staged)?.is_none(),
                    "Unexpected profile data appeared during recovery"
                );
            }
            if let Some(temporary) = &temporary
                && directory_identity(temporary)?.is_some()
            {
                fs::remove_dir(temporary)?;
                File::open(parent)?.sync_all()?;
            }
        } else if let Some(original) = restore.original {
            if current == Some(original) && failed_identity.is_none() {
                fs::rename(&live, &failed)?;
                File::open(parent)?.sync_all()?;
            } else {
                ensure!(
                    current.is_none() && failed_identity == Some(original),
                    "New preview data changed during recovery"
                );
            }
        } else {
            ensure!(
                current.is_none() && failed_identity.is_none(),
                "Unexpected preview data appeared during recovery"
            );
        }
        Ok(())
    }
}

fn profiles(registry: &Option<Registry>) -> &[ProfileMetadata] {
    registry
        .as_ref()
        .map_or(&[], |registry| registry.profiles.as_slice())
}

fn check_registry_change(
    before: &Option<Registry>,
    current: &Option<Registry>,
    new: &BrowserIdentity,
) -> Result<()> {
    if let Some(registry) = current {
        registry.validate()?;
    }
    let before = profiles(before);
    let current = profiles(current);
    ensure!(
        before.len() == current.len(),
        "Profile registry changed after the update snapshot; no further roots were restored"
    );
    for (before, current) in before.iter().zip(current) {
        ensure!(
            before.id == current.id
                && before.label == current.label
                && before.profile_schema == current.profile_schema
                && (before.last_browser == current.last_browser
                    || current.last_browser.as_ref() == Some(new)),
            "Unrecognized profile metadata changed after the update snapshot; preserve it for explicit recovery"
        );
    }
    Ok(())
}

fn validate_transaction(transaction: &str) -> Result<()> {
    ensure!(
        !transaction.is_empty()
            && transaction.len() <= 40
            && transaction.bytes().all(|byte| byte.is_ascii_digit()),
        "Invalid update snapshot transaction"
    );
    Ok(())
}

fn validate_snapshot(
    snapshot: &Snapshot,
    transaction: &str,
    old: &Product,
    new: &Product,
) -> Result<()> {
    validate_transaction(transaction)?;
    ensure!(
        snapshot.schema_version == 1
            && snapshot.transaction == transaction
            && snapshot.old.product == *old
            && snapshot.new.product == *new,
        "Unsupported or mismatched profile update snapshot"
    );
    snapshot.old.validate()?;
    snapshot.new.validate()?;
    snapshot.new.ensure_not_older_than(&snapshot.old)?;
    if let Some(registry) = &snapshot.registry {
        registry.validate()?;
        for profile in &registry.profiles {
            if let Some(previous) = &profile.last_browser {
                snapshot.old.ensure_not_older_than(previous)?;
            }
        }
    }
    ensure!(
        profiles(&snapshot.registry)
            .iter()
            .map(|profile| &profile.id)
            .eq(snapshot.managed.iter().map(|profile| &profile.id)),
        "Profile snapshot omits or duplicates registry ownership"
    );
    Ok(())
}

fn read_snapshot(
    destination: &Path,
    transaction: &str,
    old: &Product,
    new: &Product,
) -> Result<Snapshot> {
    files::existing_directory(destination)?;
    let snapshot = files::read_json(
        &destination.join("profiles.json"),
        MAX_BACKUP_METADATA_BYTES,
    )?;
    validate_snapshot(&snapshot, transaction, old, new)?;
    if !snapshot.managed.is_empty() {
        files::existing_directory(&destination.join("profiles"))?;
    }
    Ok(snapshot)
}

fn snapshot_roots<'a>(
    destination: &Path,
    snapshot: &'a Snapshot,
) -> Vec<(Root, Option<&'a [Entry]>, PathBuf)> {
    std::iter::once((
        Root::Preview,
        snapshot.preview.as_deref(),
        destination.join("profile"),
    ))
    .chain(snapshot.managed.iter().map(|profile| {
        (
            Root::Managed {
                id: profile.id.clone(),
            },
            Some(profile.entries.as_slice()),
            destination.join("profiles").join(profile.id.to_string()),
        )
    }))
    .collect()
}

fn verify_snapshot(destination: &Path, snapshot: &Snapshot) -> Result<()> {
    for (_, entries, backup) in snapshot_roots(destination, snapshot) {
        if let Some(entries) = entries {
            ensure!(
                inventory(&backup, true)?.as_slice() == entries,
                "Profile update snapshot is incomplete or corrupted; no further recovery was attempted"
            );
        }
    }
    Ok(())
}

fn directory_identity(path: &Path) -> Result<Option<DirectoryIdentity>> {
    match fs::symlink_metadata(path) {
        Ok(metadata) => {
            files::existing_directory(path)?;
            Ok(Some(DirectoryIdentity {
                device: metadata.dev(),
                inode: metadata.ino(),
            }))
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error)
            .with_context(|| format!("Cannot inspect profile recovery path {}", path.display())),
    }
}

fn sync_parents(left: &Path, right: &Path) -> Result<()> {
    File::open(left.parent().context("Missing recovery parent")?)?.sync_all()?;
    File::open(right.parent().context("Missing recovery parent")?)?.sync_all()?;
    Ok(())
}
