#![cfg(target_os = "macos")]

use anyhow::{Context, Result, ensure};
use fs2::FileExt;
use stainless_release::{FEED_NAME, Product, UpdateFeed, verify_asset};
use stainless_updater::{Locations, bundle, install};
use std::fs;
use std::path::PathBuf;

fn package(variable: &str) -> Result<(PathBuf, UpdateFeed)> {
    let package = PathBuf::from(
        std::env::var_os(variable)
            .with_context(|| format!("Set {variable} to an actual release package directory"))?,
    );
    let feed: UpdateFeed = serde_json::from_slice(&fs::read(package.join(FEED_NAME))?)?;
    feed.validate()?;
    let archive = package.join(&feed.asset.name);
    verify_asset(&archive, &feed.asset)?;
    Ok((package, feed))
}

struct UpgradeFixture {
    _directory: tempfile::TempDir,
    locations: Locations,
    previous: Product,
    update: UpdateFeed,
    archive: PathBuf,
}

impl UpgradeFixture {
    fn new() -> Result<Self> {
        let (old_package, old_feed) = package("STAINLESS_TEST_PREVIOUS_PACKAGE_DIR")?;
        let (new_package, update) = package("STAINLESS_TEST_PACKAGE_DIR")?;
        update.validate_upgrade(&old_feed.release, &install::os_version()?)?;
        let directory = tempfile::tempdir()?;
        let applications = directory.path().join("Applications");
        fs::create_dir(&applications)?;
        bundle::unpack(&old_package.join(&old_feed.asset.name), &applications)?;
        let locations = Locations {
            app: applications.join("Stainless.app"),
            state: directory.path().join("state"),
            profile: directory.path().join("synthetic-profile"),
        };
        bundle::validate(&locations.app, &old_feed.release)?;
        fs::create_dir(&locations.state)?;
        fs::create_dir(&locations.profile)?;
        fs::write(
            locations.profile.join("Bookmarks"),
            b"synthetic bookmark backup fixture",
        )?;
        Ok(Self {
            _directory: directory,
            locations,
            previous: old_feed.release,
            archive: new_package.join(&update.asset.name),
            update,
        })
    }

    fn stage_and_install(&self) -> Result<()> {
        install::stage(&self.locations, &self.update, &self.archive)?;
        ensure!(
            install::install(&self.locations)?.state == "awaiting_launch",
            "Installed payload did not enter launch confirmation"
        );
        bundle::validate(&self.locations.app, &self.update.release)?;
        Ok(())
    }
}

#[test]
#[ignore = "Requires an actual locally built release package"]
fn built_update_payload_extracts_as_a_complete_valid_app() -> Result<()> {
    let (package, feed) = package("STAINLESS_TEST_PACKAGE_DIR")?;
    let archive = package.join(&feed.asset.name);

    let extracted = tempfile::tempdir()?;
    bundle::unpack(&archive, extracted.path())?;
    bundle::validate(&extracted.path().join("Stainless.app"), &feed.release)?;
    Ok(())
}

#[test]
#[ignore = "Requires actual consecutive release payloads; startup health is mocked"]
fn real_payload_upgrade_preserves_profile_and_retains_previous_app() -> Result<()> {
    let fixture = UpgradeFixture::new()?;
    fixture.stage_and_install()?;
    ensure!(
        fs::read(fixture.locations.profile.join("Bookmarks"))?
            == b"synthetic bookmark backup fixture",
        "Atomic upgrade changed the synthetic profile",
    );
    ensure!(
        install::reconcile(&fixture.locations, false)?.state == "awaiting_launch",
        "The updater claimed health without a launch"
    );

    // Only the browser startup signal is mocked; payload checks and exchange are real.
    fs::create_dir(fixture.locations.state.join("health"))?;
    fs::write(
        fixture
            .locations
            .state
            .join("health")
            .join(fixture.update.release.build.to_string()),
        b"ready\n",
    )?;
    ensure!(
        install::reconcile(&fixture.locations, true)?.state == "healthy",
        "The simulated health signal did not confirm the new installation"
    );
    let entries = fs::read_dir(
        fixture
            .locations
            .app
            .parent()
            .context("Missing app parent")?,
    )?
    .collect::<std::io::Result<Vec<_>>>()?;
    let previous = entries
        .into_iter()
        .find(|entry| {
            entry
                .file_name()
                .to_string_lossy()
                .starts_with(".Stainless.previous-")
        })
        .context("Last known-good app was not retained")?;
    bundle::validate(&previous.path(), &fixture.previous)?;
    Ok(())
}

#[test]
#[ignore = "Requires actual consecutive release payloads; failed launch is injected"]
fn real_payload_failed_launch_restores_the_previous_app_and_profile() -> Result<()> {
    let fixture = UpgradeFixture::new()?;
    fixture.stage_and_install()?;
    fs::write(
        fixture.locations.profile.join("Bookmarks"),
        b"synthetic failed-launch mutation",
    )?;
    ensure!(
        install::reconcile(&fixture.locations, true)?.state == "rolled_back",
        "A launch without its startup marker did not recover"
    );
    bundle::validate(&fixture.locations.app, &fixture.previous)?;
    ensure!(
        fs::read(fixture.locations.profile.join("Bookmarks"))?
            == b"synthetic bookmark backup fixture",
        "Recovery did not restore the original synthetic profile",
    );
    ensure!(
        install::reject_failed_build(&fixture.locations, fixture.update.release.build).is_err(),
        "The failed payload would be retried unattended"
    );
    Ok(())
}

#[test]
#[ignore = "Requires actual consecutive release payloads"]
fn real_payload_replacement_is_blocked_while_the_browser_lock_is_held() -> Result<()> {
    let fixture = UpgradeFixture::new()?;
    install::stage(&fixture.locations, &fixture.update, &fixture.archive)?;
    let lock = fixture.locations.lock("install.lock")?;
    FileExt::lock_shared(&lock)?;
    ensure!(
        install::install(&fixture.locations).is_err(),
        "The updater replaced an app with a live browser lock"
    );
    bundle::validate(&fixture.locations.app, &fixture.previous)?;
    Ok(())
}

#[test]
#[ignore = "Requires actual consecutive release payloads"]
fn real_payload_bad_hash_and_wrong_target_leave_the_installation_untouched() -> Result<()> {
    let fixture = UpgradeFixture::new()?;
    let mut corrupt = fixture.update.clone();
    corrupt.asset.sha256 = "0".repeat(64);
    ensure!(
        install::stage(&fixture.locations, &corrupt, &fixture.archive).is_err(),
        "The updater accepted a mismatched final asset hash"
    );
    let mut wrong_target = fixture.update.clone();
    wrong_target.arch = "x86_64".into();
    ensure!(
        install::stage(&fixture.locations, &wrong_target, &fixture.archive).is_err(),
        "The updater accepted a payload for another architecture"
    );
    ensure!(
        !fixture.locations.state.join("ready.json").exists(),
        "Invalid metadata produced a staged-update marker"
    );
    bundle::validate(&fixture.locations.app, &fixture.previous)?;
    Ok(())
}
