pub mod bundle;
pub mod install;
pub mod network;

use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
pub use stainless_profiles::files::write_json;
use std::fs::{self, File};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug)]
pub struct Locations {
    pub app: PathBuf,
    pub state: PathBuf,
    pub profile: PathBuf,
}

impl Locations {
    pub fn for_app(app: &Path) -> Result<Self> {
        let home = PathBuf::from(std::env::var_os("HOME").context("HOME is unset")?);
        ensure!(home.is_absolute(), "HOME must be absolute");
        let app = fs::canonicalize(app).context("Cannot locate the installed Stainless.app")?;
        ensure!(
            app.file_name().is_some_and(|name| name == "Stainless.app"),
            "Unexpected app name"
        );
        ensure!(
            app.starts_with("/Applications") || app.starts_with(home.join("Applications")),
            "Automatic updates require Stainless.app in /Applications or ~/Applications"
        );
        let state = home.join("Library/Application Support/Stainless Updater");
        fs::create_dir_all(&state)?;
        ensure!(
            !state.symlink_metadata()?.file_type().is_symlink(),
            "Updater state cannot be a symlink"
        );
        fs::set_permissions(&state, fs::Permissions::from_mode(0o700))?;
        Ok(Self {
            app,
            state,
            profile: home.join("Library/Application Support/Stainless"),
        })
    }

    pub fn lock(&self, name: &str) -> Result<File> {
        ensure!(
            matches!(name, "state.lock" | "install.lock"),
            "Invalid lock name"
        );
        stainless_profiles::files::open_lock(&self.state.join(name))
            .context("Cannot open updater lock")
    }
}

#[derive(Debug, Deserialize, Serialize)]
pub struct Status {
    pub state: String,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
}

impl Status {
    pub fn new(state: &str, message: impl Into<String>) -> Self {
        Self {
            state: state.into(),
            message: message.into(),
            version: None,
        }
    }

    pub fn for_release(
        state: &str,
        message: impl Into<String>,
        release: &stainless_release::Product,
    ) -> Self {
        Self {
            version: Some(release.version.to_string()),
            ..Self::new(state, message)
        }
    }
}
