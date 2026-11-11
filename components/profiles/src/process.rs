use crate::modes::{LaunchRequest, Mode};
use crate::{BrowserIdentity, ProfileId, Store, app, chromium_version, files};
use anyhow::{Context, Result, ensure};
use std::fs::{self, File};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

impl Store {
    pub fn run(&self, id: &ProfileId, app_path: &Path) -> Result<()> {
        self.launch(id, app_path, &LaunchRequest::new(Mode::Standard, None)?)
    }

    pub fn launch(&self, id: &ProfileId, app_path: &Path, request: &LaunchRequest) -> Result<()> {
        self.launch_owned(id, app_path, request, || Ok(()), Stdio::inherit())
    }

    pub fn launch_observed(
        &self,
        id: &ProfileId,
        app_path: &Path,
        request: &LaunchRequest,
        started: impl FnOnce() -> Result<()>,
    ) -> Result<()> {
        self.launch_owned(id, app_path, request, started, Stdio::null())
    }

    fn launch_owned(
        &self,
        id: &ProfileId,
        app_path: &Path,
        request: &LaunchRequest,
        started: impl FnOnce() -> Result<()>,
        stdout: Stdio,
    ) -> Result<()> {
        request.mode().require_available()?;
        let _installation = self.installation_lock(false)?;
        self.check_update_transaction()?;
        let (app, browser) = app::identity(app_path)?;
        match request.mode() {
            Mode::Standard => {
                let (data, cache, _owner) = self.prepare_standard(id, &browser)?;
                run_child(&app, &data, &cache, request, &mut true, started, stdout)
            }
            Mode::Incognito => {
                app::require_managed_modes(&app)?;
                let registry_lock = self.registry_lock(false)?;
                ensure!(
                    self.registry()?
                        .profiles
                        .iter()
                        .any(|profile| profile.id == *id),
                    "Unknown profile"
                );
                let sessions = self.profile_directory(id)?.join("incognito");
                files::directory(&sessions, true)?;
                let session = tempfile::Builder::new()
                    .prefix("s-")
                    .tempdir_in(&sessions)?;
                drop(registry_lock);
                let path = session.path().to_owned();
                let mut quiescent = true;
                let result = (|| {
                    let data = path.join("data");
                    let cache = path.join("cache");
                    files::directory(&data, true)?;
                    files::directory(&cache, true)?;
                    run_child(
                        &app,
                        &data,
                        &cache,
                        request,
                        &mut quiescent,
                        started,
                        stdout,
                    )
                })();
                if !quiescent {
                    let retained = session.keep();
                    return result.with_context(|| {
                        format!(
                            "Browser ownership is uncertain; private session retained at {}. Close the browser before explicit cleanup",
                            retained.display()
                        )
                    });
                }
                if let Err(error) = session.close() {
                    if let Err(browser_error) = &result {
                        eprintln!("Managed Incognito browser failed: {browser_error:#}");
                    }
                    return Err(error).with_context(|| {
                        format!(
                            "Cannot remove the private session at {}; close its browser and remove that exact directory before sharing this account",
                            path.display()
                        )
                    });
                }
                result
            }
            _ => anyhow::bail!("Browsing mode is unavailable"),
        }
    }

    fn prepare_standard(
        &self,
        id: &ProfileId,
        browser: &BrowserIdentity,
    ) -> Result<(PathBuf, PathBuf, File)> {
        let registry_lock = self.registry_lock(true)?;
        let mut registry = self.registry()?;
        let profile = registry
            .profiles
            .iter_mut()
            .find(|profile| profile.id == *id)
            .context("Unknown profile")?;
        let owner = self.profile_lock(id)?;
        if let Some(previous) = &profile.last_browser {
            browser.ensure_not_older_than(previous)?;
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
        Ok((data, cache, owner))
    }
}

fn run_child(
    app: &Path,
    data: &Path,
    cache: &Path,
    request: &LaunchRequest,
    quiescent: &mut bool,
    started: impl FnOnce() -> Result<()>,
    stdout: Stdio,
) -> Result<()> {
    let mut command = Command::new(app.join("Contents/MacOS/Stainless"));
    command
        .arg(format!("--user-data-dir={}", data.display()))
        .arg(format!("--disk-cache-dir={}", cache.display()))
        .arg(format!(
            "--stainless-managed-mode={}",
            request.mode().name()
        ));
    if request.mode() == Mode::Incognito {
        command.args(["--incognito", "--disable-extensions", "--no-first-run"]);
    }
    if let Some(url) = request.url() {
        command.arg("--").arg(url.as_str());
    }
    let mut child = command
        .stdin(Stdio::null())
        .stdout(stdout)
        .spawn()
        .context("Cannot start the owned browser; any recorded compatibility floor was retained")?;
    *quiescent = false;
    let notification = started();
    if let Err(error) = &notification {
        eprintln!("Cannot report owned browser start; retaining ownership until exit: {error:#}");
    }
    let status = child
        .wait()
        .context("Cannot wait for the owned browser process")?;
    *quiescent = true;
    notification?;
    ensure!(
        status.success(),
        "Profile browser exited unsuccessfully: {status}"
    );
    Ok(())
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
