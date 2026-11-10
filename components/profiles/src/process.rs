use crate::{ProfileId, Store, app, chromium_version, files};
use anyhow::{Context, Result, ensure};
use std::fs;
use std::io::Read;
use std::path::Path;
use std::process::{Command, Stdio};

impl Store {
    pub fn run(&self, id: &ProfileId, app_path: &Path) -> Result<()> {
        let _installation = self.installation_lock(false)?;
        self.check_update_transaction()?;
        let (app, browser) = app::identity(app_path)?;
        let registry_lock = self.registry_lock(true)?;
        let mut registry = self.registry()?;
        let profile = registry
            .profiles
            .iter_mut()
            .find(|profile| profile.id == *id)
            .context("Unknown profile")?;
        let _owner = self.profile_lock(id)?;
        if let Some(previous) = &profile.last_browser {
            ensure!(
                browser.product.build >= previous.product.build
                    && browser.product.version >= previous.product.version
                    && chromium_version(&browser.chromium_version)?
                        >= chromium_version(&previous.chromium_version)?,
                "Refusing a browser downgrade on live profile data; restore a matching backup into a new profile"
            );
        }
        let directory = self.profile_directory(id)?;
        let data = directory.join("data");
        check_singleton(&data)?;
        match last_version(&data)? {
            Some(version) => ensure!(
                chromium_version(&browser.chromium_version)? >= chromium_version(&version)?,
                "The profile was written by a newer Chromium; restore its matching backup instead"
            ),
            None => ensure!(
                fs::read_dir(&data)?.next().is_none() || profile.last_browser.is_some(),
                "Nonempty profile has no recorded browser identity; explicit migration is required"
            ),
        }
        let cache = directory.join("cache");
        files::directory(&cache, true)?;
        // Record the compatibility floor before the browser can mutate its stores.
        profile.last_browser = Some(browser.clone());
        files::write_json_limited(
            &self.root.join("registry.json"),
            &registry,
            crate::MAX_REGISTRY_BYTES,
        )?;
        drop(registry_lock);
        let mut child = Command::new(app.join("Contents/MacOS/Stainless"))
            .arg(format!("--user-data-dir={}", data.display()))
            .arg(format!("--disk-cache-dir={}", cache.display()))
            .stdin(Stdio::null())
            .spawn()
            .context("Cannot start this profile's browser; its compatibility floor was retained")?;
        let status = child
            .wait()
            .context("Cannot wait for the owned browser process")?;
        ensure!(
            status.success(),
            "Profile browser exited unsuccessfully: {status}"
        );
        Ok(())
    }
}

pub(crate) fn check_singleton(data: &Path) -> Result<()> {
    match fs::symlink_metadata(data.join("SingletonLock")) {
        Ok(_) => anyhow::bail!(
            "Chromium singleton ownership is present; quit the browser, or inspect stale state before retrying"
        ),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error).context("Cannot inspect Chromium singleton ownership"),
    }
}

pub(crate) fn last_version(data: &Path) -> Result<Option<String>> {
    let path = data.join("Last Version");
    match fs::symlink_metadata(&path) {
        Ok(_) => {
            let mut version = String::new();
            files::open_file(&path)?
                .take(65)
                .read_to_string(&mut version)?;
            ensure!(version.len() <= 64, "Oversized Chromium profile version");
            let version = version.trim().to_owned();
            chromium_version(&version)?;
            Ok(Some(version))
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error).context("Cannot inspect Chromium profile version"),
    }
}
