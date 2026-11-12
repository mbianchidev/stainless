pub mod app;
mod backups;
pub mod files;
pub mod modes;
pub mod native;
mod process;
pub mod protocol;
pub mod updates;
pub use backups::{BackupId, ImportedProfile};

use anyhow::{Context, Result, ensure};
use fs2::FileExt;
use serde::{Deserialize, Serialize};
use stainless_release::Product;
use std::collections::HashSet;
use std::fmt;
use std::fs::{self, File};
use std::path::{Path, PathBuf};

pub const REGISTRY_VERSION: u32 = 1;
pub const PROFILE_SCHEMA: u32 = 1;
const MAX_PROFILES: usize = 256;
const MAX_REGISTRY_BYTES: u64 = 1024 * 1024;

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq, Hash)]
#[serde(try_from = "String", into = "String")]
pub struct ProfileId(String);

impl TryFrom<String> for ProfileId {
    type Error = anyhow::Error;

    fn try_from(value: String) -> Result<Self> {
        validate_id(&value, "p-")?;
        Ok(Self(value))
    }
}

impl From<ProfileId> for String {
    fn from(value: ProfileId) -> Self {
        value.0
    }
}

impl fmt::Display for ProfileId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

fn validate_id(value: &str, prefix: &str) -> Result<()> {
    ensure!(
        value.len() == 18
            && value.starts_with(prefix)
            && value[2..].bytes().all(|byte| byte.is_ascii_alphanumeric()),
        "Invalid profile/backup identifier"
    );
    Ok(())
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ProfileMetadata {
    pub id: ProfileId,
    pub label: String,
    pub profile_schema: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_browser: Option<BrowserIdentity>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct BrowserIdentity {
    pub product: Product,
    pub chromium_version: String,
}

impl BrowserIdentity {
    pub fn validate(&self) -> Result<()> {
        self.product.validate()?;
        ensure!(
            self.product.profile_schema == PROFILE_SCHEMA,
            "Only the unchanged preview profile schema is supported"
        );
        chromium_version(&self.chromium_version)?;
        Ok(())
    }

    pub(crate) fn ensure_not_older_than(&self, previous: &Self) -> Result<()> {
        ensure!(
            self.product.build >= previous.product.build
                && self.product.version >= previous.product.version
                && chromium_version(&self.chromium_version)?
                    >= chromium_version(&previous.chromium_version)?,
            "Refusing a browser downgrade on live profile data; restore a matching backup into a new profile"
        );
        Ok(())
    }
}

fn chromium_version(value: &str) -> Result<[u32; 4]> {
    let parts: Vec<_> = value.split('.').collect();
    ensure!(
        value.len() <= 64
            && parts.len() == 4
            && parts
                .iter()
                .all(|part| !part.is_empty() && part.bytes().all(|byte| byte.is_ascii_digit())),
        "Invalid Chromium profile version"
    );
    Ok([
        parts[0].parse()?,
        parts[1].parse()?,
        parts[2].parse()?,
        parts[3].parse()?,
    ])
}

impl ProfileMetadata {
    fn validate(&self) -> Result<()> {
        validate_id(&self.id.0, "p-")?;
        validate_label(&self.label)?;
        ensure!(
            self.profile_schema == PROFILE_SCHEMA,
            "Unsupported profile data schema; no migration or downgrade is authorized"
        );
        if let Some(browser) = &self.last_browser {
            browser.validate()?;
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct Registry {
    schema_version: u32,
    profiles: Vec<ProfileMetadata>,
}

impl Registry {
    fn validate(&self) -> Result<()> {
        ensure!(
            self.schema_version == REGISTRY_VERSION,
            "Unsupported profile registry version; leave it untouched and use a compatible tool"
        );
        ensure!(self.profiles.len() <= MAX_PROFILES, "Too many profiles");
        let mut ids = HashSet::new();
        for profile in &self.profiles {
            profile.validate()?;
            ensure!(ids.insert(&profile.id), "Duplicate profile ownership");
        }
        Ok(())
    }
}

pub struct Store {
    root: PathBuf,
}

impl Store {
    pub fn for_home(home: &Path) -> Result<Self> {
        let store = Self::at_home(home)?;
        store.initialize()?;
        Ok(store)
    }

    fn at_home(home: &Path) -> Result<Self> {
        ensure!(home.is_absolute(), "Profile HOME must be absolute");
        let home = fs::canonicalize(home).context("Cannot locate profile HOME")?;
        let library = home.join("Library");
        files::directory(&library, false)?;
        let support = library.join("Application Support");
        files::directory(&support, false)?;
        Ok(Self {
            root: support.join("Stainless Profiles"),
        })
    }

    fn initialize(&self) -> Result<()> {
        let _installation = self.installation_lock(false)?;
        match fs::symlink_metadata(self.root.join("registry.json")) {
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                self.check_update_transaction()?;
            }
            Err(error) => return Err(error).context("Cannot inspect profile registry"),
        }
        files::directory(&self.root, true)?;
        files::directory(&self.root.join("profiles"), true)?;
        files::directory(&self.root.join("backups"), true)?;
        let _lock = self.registry_lock(true)?;
        let registry_path = self.root.join("registry.json");
        match fs::symlink_metadata(&registry_path) {
            Ok(_) => {
                self.registry()?;
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                ensure!(
                    fs::read_dir(self.root.join("profiles"))?.next().is_none()
                        && fs::read_dir(self.root.join("backups"))?.next().is_none(),
                    "Registry is missing but profile/backup data exists; preserve it for explicit recovery"
                );
                files::write_json_limited(
                    &registry_path,
                    &Registry {
                        schema_version: REGISTRY_VERSION,
                        profiles: Vec::new(),
                    },
                    MAX_REGISTRY_BYTES,
                )?;
            }
            Err(error) => return Err(error).context("Cannot inspect profile registry"),
        }
        Ok(())
    }

    pub fn create(&self, label: &str) -> Result<ProfileMetadata> {
        validate_label(label)?;
        let _installation = self.installation_lock(false)?;
        self.check_update_transaction()?;
        let _lock = self.registry_lock(true)?;
        let registry = self.registry_for_creation()?;
        let (profile, directory) = self.allocate_profile(label)?;
        files::directory(&directory.join("data"), true)?;
        self.register_profile(registry, profile, &directory)
    }

    fn registry_for_creation(&self) -> Result<Registry> {
        let registry = self.registry()?;
        ensure!(
            registry.profiles.len() < MAX_PROFILES,
            "Profile registry is full"
        );
        Ok(registry)
    }

    fn allocate_profile(&self, label: &str) -> Result<(ProfileMetadata, PathBuf)> {
        validate_label(label)?;
        files::existing_directory(&self.root.join("profiles"))?;
        let directory = tempfile::Builder::new()
            .prefix("p-")
            .rand_bytes(16)
            .tempdir_in(self.root.join("profiles"))?
            .keep();
        let id = ProfileId::try_from(
            directory
                .file_name()
                .and_then(|name| name.to_str())
                .context("Invalid allocated profile identifier")?
                .to_owned(),
        )?;
        let profile = ProfileMetadata {
            id,
            label: label.to_owned(),
            profile_schema: PROFILE_SCHEMA,
            last_browser: None,
        };
        Ok((profile, directory))
    }

    fn register_profile(
        &self,
        mut registry: Registry,
        profile: ProfileMetadata,
        directory: &Path,
    ) -> Result<ProfileMetadata> {
        files::existing_directory(&directory.join("data"))?;
        File::open(directory)?.sync_all()?;
        File::open(self.root.join("profiles"))?.sync_all()?;
        registry.profiles.push(profile.clone());
        registry.validate()?;
        files::write_json_limited(
            &self.root.join("registry.json"),
            &registry,
            MAX_REGISTRY_BYTES,
        )
        .with_context(|| {
            format!(
                "Cannot register profile {}; its directory is retained at {}",
                profile.id,
                directory.display()
            )
        })?;
        Ok(profile)
    }

    pub fn list(&self) -> Result<Vec<ProfileMetadata>> {
        let _lock = self.registry_lock(false)?;
        Ok(self.registry()?.profiles)
    }

    fn registry(&self) -> Result<Registry> {
        let registry: Registry =
            files::read_json(&self.root.join("registry.json"), MAX_REGISTRY_BYTES)?;
        registry.validate()?;
        Ok(registry)
    }

    fn registry_lock(&self, exclusive: bool) -> Result<File> {
        files::existing_directory(&self.root)?;
        let lock = files::open_lock(&self.root.join("registry.lock"))?;
        if exclusive {
            FileExt::lock_exclusive(&lock)
        } else {
            FileExt::lock_shared(&lock)
        }
        .context("Another profile registry operation is in progress")?;
        Ok(lock)
    }

    fn profile_directory(&self, id: &ProfileId) -> Result<PathBuf> {
        validate_id(&id.0, "p-")?;
        let directory = self.root.join("profiles").join(&id.0);
        files::existing_directory(&self.root.join("profiles"))?;
        files::existing_directory(&directory)?;
        files::existing_directory(&directory.join("data"))?;
        Ok(directory)
    }

    fn profile_lock(&self, id: &ProfileId) -> Result<File> {
        let directory = self.profile_directory(id)?;
        let lock = files::open_lock(&directory.join("owner.lock"))?;
        lock.try_lock_exclusive()
            .context("This profile is already owned by a browser or maintenance operation")?;
        Ok(lock)
    }

    fn installation_lock(&self, exclusive: bool) -> Result<File> {
        let state = self.root.with_file_name("Stainless Updater");
        files::directory(&state, true)?;
        let lock = files::open_lock(&state.join("install.lock"))?;
        if exclusive {
            lock.try_lock_exclusive()
        } else {
            FileExt::try_lock_shared(&lock)
        }
        .context("Quit every Stainless browser and finish any update before profile maintenance")?;
        Ok(lock)
    }
}

fn validate_label(label: &str) -> Result<()> {
    ensure!(
        !label.trim().is_empty() && label.len() <= 128 && !label.chars().any(char::is_control),
        "Profile label must contain 1-128 bytes of printable text"
    );
    Ok(())
}

#[cfg(test)]
mod tests;
