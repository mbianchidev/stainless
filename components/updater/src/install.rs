use crate::{Locations, Status, bundle, write_json};
use anyhow::{Context, Result, ensure};
use fs2::FileExt;
use serde::{Deserialize, Serialize};
use stainless_release::{Product, UpdateFeed, verify_asset};
use std::ffi::CString;
use std::fs::{self, File};
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Transaction {
    id: String,
    app: PathBuf,
    old: Product,
    new: Product,
    phase: Phase,
    observed_launch: bool,
    had_profile: bool,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum Phase {
    Prepared,
    Installed,
    Healthy,
    RolledBack,
}

struct Preparation {
    incoming: PathBuf,
    snapshot_parent: PathBuf,
    journaled: bool,
}

impl Drop for Preparation {
    fn drop(&mut self) {
        if self.journaled {
            return;
        }
        for path in [&self.incoming, &self.snapshot_parent] {
            if path.exists()
                && let Err(error) = fs::remove_dir_all(path)
            {
                eprintln!(
                    "Cannot clean interrupted update preparation {}: {error}",
                    path.display()
                );
            }
        }
    }
}

impl Transaction {
    fn incoming(&self) -> PathBuf {
        self.app
            .with_file_name(format!(".Stainless.incoming-{}.app", self.id))
    }
    fn previous(&self) -> PathBuf {
        self.app
            .with_file_name(format!(".Stainless.previous-{}.app", self.id))
    }
    fn snapshot(&self, locations: &Locations) -> PathBuf {
        locations
            .state
            .join("backups")
            .join(&self.id)
            .join("profile")
    }
    fn validate(&self, locations: &Locations) -> Result<()> {
        ensure!(
            self.app == locations.app,
            "Recovery belongs to a different application"
        );
        ensure!(
            !self.id.is_empty()
                && self.id.len() <= 40
                && self.id.bytes().all(|byte| byte.is_ascii_digit()),
            "Invalid recovery transaction identifier"
        );
        self.old.validate()?;
        self.new.validate()?;
        ensure!(
            self.old.profile_schema == self.new.profile_schema,
            "Unsupported recovery schema"
        );
        Ok(())
    }
}

pub fn status(locations: &Locations) -> Result<Status> {
    let path = locations.state.join("status.json");
    let previous = if path.exists() {
        serde_json::from_slice::<Status>(&fs::read(path)?)?
    } else {
        Status::new(
            "idle",
            "Automatic updates need read-only private release authorization.",
        )
    };
    let journal = locations.state.join("transaction.json");
    let transaction = if journal.exists() {
        let transaction: Transaction = serde_json::from_slice(&fs::read(journal)?)?;
        transaction.validate(locations)?;
        Some(transaction)
    } else {
        None
    };
    if let Some(transaction) = &transaction
        && matches!(transaction.phase, Phase::Prepared | Phase::Installed)
    {
        return Ok(Status::for_release(
            "awaiting_launch",
            "An update transaction is awaiting startup confirmation or recovery.",
            &transaction.new,
        ));
    }
    let ready = locations.state.join("ready.json");
    if ready.exists() {
        let feed: UpdateFeed = serde_json::from_slice(&fs::read(ready)?)?;
        feed.validate_upgrade(&bundle::product(&locations.app)?, &os_version()?)?;
        let app = locations
            .state
            .join("staged")
            .join(feed.release.build.to_string())
            .join("Stainless.app");
        ensure!(
            bundle::product(&app)? == feed.release,
            "Cached update identity does not match its feed"
        );
        let mut message =
            "A previously verified update is staged and can install without network access."
                .to_owned();
        if previous.state == "error" {
            message.push_str(&format!(" Last operation failed: {}", previous.message));
        }
        return Ok(Status::for_release("staged", message, &feed.release));
    }
    if previous.state == "error" {
        return Ok(previous);
    }
    if let Some(transaction) = transaction {
        return Ok(match transaction.phase {
            Phase::Healthy => Status::for_release(
                "healthy",
                "Updated browser confirmed healthy; previous app/profile backup retained.",
                &transaction.new,
            ),
            Phase::RolledBack => Status::for_release(
                "rolled_back",
                "Previous app/profile restored after an unsuccessful update; failed state retained.",
                &transaction.old,
            ),
            _ => anyhow::bail!("Unexpected nonterminal transaction"),
        });
    }
    Ok(previous)
}

fn save_recovery_status(locations: &Locations, status: Status) -> Result<Status> {
    write_json(&locations.state.join("status.json"), &status)?;
    Ok(status)
}

pub fn stage(locations: &Locations, feed: &UpdateFeed, archive: &Path) -> Result<Status> {
    stage_with(locations, feed, archive, bundle::validate)
}

fn stage_with(
    locations: &Locations,
    feed: &UpdateFeed,
    archive: &Path,
    validate: impl Fn(&Path, &Product) -> Result<()>,
) -> Result<Status> {
    verify_asset(archive, &feed.asset)?;
    let current = bundle::product(&locations.app)?;
    feed.validate_upgrade(&current, &os_version()?)?;
    let staging = locations.state.join("staged");
    fs::create_dir_all(&staging)?;
    let destination = staging.join(feed.release.build.to_string());
    let temporary = tempfile::tempdir_in(&staging)?;
    bundle::unpack(archive, temporary.path())?;
    validate(&temporary.path().join("Stainless.app"), &feed.release)?;
    if destination.exists() {
        swap(temporary.path(), &destination)?;
    } else {
        fs::rename(temporary.path(), &destination)?;
    }
    File::open(&staging)?.sync_all()?;
    write_json(&locations.state.join("ready.json"), feed)?;
    let status = Status::for_release(
        "staged",
        "Update verified and staged. Restart Stainless when ready.",
        &feed.release,
    );
    Ok(status)
}

pub fn install(locations: &Locations) -> Result<Status> {
    install_with(locations, bundle::validate)
}

fn install_with(
    locations: &Locations,
    validate: impl Fn(&Path, &Product) -> Result<()>,
) -> Result<Status> {
    let lock = locations.lock("install.lock")?;
    lock.try_lock_exclusive()
        .context("Stainless is still running. Quit every Stainless window/profile before installing; no files were replaced")?;
    let transaction_path = locations.state.join("transaction.json");
    if transaction_path.exists() {
        let transaction: Transaction = serde_json::from_slice(&fs::read(&transaction_path)?)?;
        transaction.validate(locations)?;
        ensure!(
            matches!(transaction.phase, Phase::Healthy | Phase::RolledBack),
            "An earlier update needs recovery/health confirmation before another installation"
        );
    }
    let feed: UpdateFeed = serde_json::from_slice(
        &fs::read(locations.state.join("ready.json")).context("No verified update is staged")?,
    )?;
    let current = bundle::product(&locations.app)?;
    feed.validate_upgrade(&current, &os_version()?)?;
    let staged = locations
        .state
        .join("staged")
        .join(feed.release.build.to_string())
        .join("Stainless.app");
    validate(&staged, &feed.release)?;
    let id = SystemTime::now()
        .duration_since(UNIX_EPOCH)?
        .as_nanos()
        .to_string();
    let transaction = Transaction {
        id,
        app: locations.app.clone(),
        old: current,
        new: feed.release.clone(),
        phase: Phase::Prepared,
        observed_launch: false,
        had_profile: locations.profile.exists(),
    };
    ensure!(
        !locations
            .profile
            .symlink_metadata()
            .is_ok_and(|metadata| metadata.file_type().is_symlink()),
        "Automatic profile backups cannot use a symlinked profile root"
    );
    ensure!(
        !transaction.had_profile || locations.profile.is_dir(),
        "Profile root is not a directory"
    );
    let snapshot = transaction.snapshot(locations);
    let snapshot_parent = snapshot
        .parent()
        .context("Missing backup parent")?
        .to_owned();
    ensure!(
        !transaction.incoming().exists() && !snapshot_parent.exists(),
        "Update preparation paths already exist; no existing data was overwritten"
    );
    let mut preparation = Preparation {
        incoming: transaction.incoming(),
        snapshot_parent: snapshot_parent.clone(),
        journaled: false,
    };
    bundle::copy_tree(&staged, &transaction.incoming())
        .context("Cannot stage beside the installed app. Check write permission in Applications and available disk space; ~/Applications is supported without administrator access")?;
    fs::create_dir_all(&snapshot_parent)?;
    if transaction.had_profile {
        bundle::copy_tree(&locations.profile, &snapshot)?;
    }
    let health = locations
        .state
        .join("health")
        .join(transaction.new.build.to_string());
    if health.exists() {
        fs::remove_file(health)?;
    }
    write_json(&transaction_path, &transaction)?;
    preparation.journaled = true;
    swap(&locations.app, &transaction.incoming())?;
    File::open(locations.app.parent().context("Missing app parent")?)?.sync_all()?;
    fs::rename(transaction.incoming(), transaction.previous())?;
    let transaction = Transaction {
        phase: Phase::Installed,
        ..transaction
    };
    write_json(&transaction_path, &transaction)?;
    Ok(Status::new(
        "awaiting_launch",
        "Update installed. The previous app and profile backup are retained until a healthy launch is confirmed.",
    ))
}

pub fn reconcile(locations: &Locations, observed_start: bool) -> Result<Status> {
    let path = locations.state.join("transaction.json");
    if !path.exists() {
        return Ok(Status::new(
            "idle",
            "No update installation needs recovery.",
        ));
    }
    let mut transaction: Transaction = serde_json::from_slice(&fs::read(&path)?)?;
    transaction.validate(locations)?;
    if matches!(transaction.phase, Phase::Healthy | Phase::RolledBack) {
        return Ok(Status::for_release(
            if transaction.phase == Phase::Healthy {
                "healthy"
            } else {
                "rolled_back"
            },
            if transaction.phase == Phase::Healthy {
                "Updated browser confirmed healthy."
            } else {
                "Previous app/profile restored after an unsuccessful update. Failed state was retained."
            },
            if transaction.phase == Phase::Healthy {
                &transaction.new
            } else {
                &transaction.old
            },
        ));
    }
    let installed = bundle::product(&locations.app)?;
    if installed == transaction.old {
        transaction.phase = Phase::RolledBack;
        write_json(&path, &transaction)?;
        return save_recovery_status(
            locations,
            Status::for_release(
                "rolled_back",
                "Interrupted update left the original browser in place.",
                &transaction.old,
            ),
        );
    }
    ensure!(
        installed == transaction.new,
        "Unexpected installed version; automatic recovery stopped"
    );
    if observed_start {
        transaction.observed_launch = true;
        write_json(&path, &transaction)?;
    }
    let health = locations
        .state
        .join("health")
        .join(transaction.new.build.to_string());
    if health.is_file() {
        ensure!(
            fs::read_to_string(&health)? == "ready\n",
            "Malformed browser startup health signal"
        );
        if !transaction.previous().exists() && transaction.incoming().exists() {
            ensure!(
                bundle::product(&transaction.incoming())? == transaction.old,
                "Interrupted replacement has an unexpected backup identity"
            );
            fs::rename(transaction.incoming(), transaction.previous())?;
        }
        transaction.phase = Phase::Healthy;
        write_json(&path, &transaction)?;
        let ready = locations.state.join("ready.json");
        if ready.exists() {
            fs::remove_file(ready)?;
        }
        return save_recovery_status(
            locations,
            Status::for_release(
                "healthy",
                "Updated browser confirmed healthy. Previous app/profile backup retained.",
                &transaction.new,
            ),
        );
    }
    let lock = locations.lock("install.lock")?;
    if let Err(error) = lock.try_lock_exclusive() {
        if error.kind() == std::io::ErrorKind::WouldBlock {
            return Ok(Status::new(
                "awaiting_health",
                "Waiting for browser startup confirmation. If startup hangs, quit Stainless to recover the retained previous app/profile.",
            ));
        }
        return Err(error).context("Cannot acquire the installation recovery lock");
    }
    if !transaction.observed_launch {
        return Ok(Status::new(
            "awaiting_launch",
            "Update installed; waiting for its first launch.",
        ));
    }
    let previous = if transaction.previous().exists() {
        transaction.previous()
    } else {
        transaction.incoming()
    };
    ensure!(
        bundle::product(&previous)? == transaction.old,
        "Previous app does not match the recovery journal; no files were replaced"
    );
    restore_profile(locations, &transaction)?;
    swap(&locations.app, &previous)?;
    transaction.phase = Phase::RolledBack;
    write_json(&path, &transaction)?;
    let ready = locations.state.join("ready.json");
    if ready.exists() {
        fs::remove_file(ready)?;
    }
    save_recovery_status(
        locations,
        Status::for_release(
            "rolled_back",
            "New browser did not complete startup. Previous app/profile restored; failed app/profile retained for inspection.",
            &transaction.old,
        ),
    )
}

fn restore_profile(locations: &Locations, transaction: &Transaction) -> Result<()> {
    let failed = locations
        .profile
        .with_file_name(format!(".Stainless.failed-profile-{}", transaction.id));
    if transaction.had_profile {
        let snapshot = transaction.snapshot(locations);
        ensure!(
            snapshot.is_dir(),
            "Profile backup is missing; refusing a potentially incompatible rollback"
        );
        let parent = locations
            .profile
            .parent()
            .context("Missing profile parent")?;
        let temporary = tempfile::tempdir_in(parent)?;
        let restored = temporary.path().join("profile");
        bundle::copy_tree(&snapshot, &restored)?;
        let retained = temporary.keep();
        if locations.profile.exists() {
            swap(&locations.profile, &restored)?;
            let failed = if failed.exists() {
                locations.profile.with_file_name(format!(
                    ".Stainless.failed-profile-{}-retry",
                    transaction.id
                ))
            } else {
                failed
            };
            fs::rename(&restored, &failed).with_context(|| {
                format!(
                    "Failed profile retained at {}; no recovery data was deleted",
                    restored.display()
                )
            })?;
        } else {
            fs::rename(restored, &locations.profile)?;
        }
        fs::remove_dir(retained)?;
    } else if locations.profile.exists() {
        ensure!(
            !failed.exists(),
            "Failed-profile recovery destination already exists"
        );
        fs::rename(&locations.profile, failed)?;
    }
    Ok(())
}

pub fn reject_failed_build(locations: &Locations, build: u64) -> Result<()> {
    let path = locations.state.join("transaction.json");
    if path.exists() {
        let transaction: Transaction = serde_json::from_slice(&fs::read(path)?)?;
        transaction.validate(locations)?;
        ensure!(
            transaction.phase != Phase::RolledBack
                || !transaction.observed_launch
                || transaction.new.build != build,
            "This preview failed its previous launch and was rolled back. Wait for a newer preview; the previous app and failed state are retained"
        );
    }
    Ok(())
}

fn swap(left: &Path, right: &Path) -> Result<()> {
    let left = CString::new(left.as_os_str().as_bytes())?;
    let right = CString::new(right.as_os_str().as_bytes())?;
    #[cfg(target_os = "macos")]
    {
        // Both directories stay addressable even if power is lost during replacement.
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

pub fn os_version() -> Result<String> {
    let output = std::process::Command::new("/usr/bin/sw_vers")
        .arg("-productVersion")
        .output()?;
    ensure!(
        output.status.success(),
        "Cannot determine the runtime macOS version"
    );
    Ok(String::from_utf8(output.stdout)?.trim().to_owned())
}

#[cfg(all(test, target_os = "macos"))]
mod tests {
    use super::*;
    use semver::Version;
    use stainless_release::Asset;

    fn product(build: u64) -> Product {
        Product {
            version: Version::parse(&format!("0.1.0-preview.{build}")).unwrap(),
            build,
            channel: "preview".into(),
            profile_schema: 1,
        }
    }

    fn fake_app(app: &Path, build: u64) {
        fs::create_dir_all(app.join("Contents")).unwrap();
        let data = serde_json::json!({
            "CFBundleIdentifier": "dev.stainless.browser",
            "CFBundleExecutable": "Stainless",
            "CrProductDirName": "Stainless",
            "StainlessVersion": product(build).version.to_string(),
            "StainlessBuild": build,
            "StainlessChannel": "preview",
            "StainlessProfileSchema": 1,
        });
        plist::to_file_xml(app.join("Contents/Info.plist"), &data).unwrap();
    }

    fn fixture(root: &Path) -> Locations {
        let locations = Locations {
            app: root.join("Applications/Stainless.app"),
            state: root.join("state"),
            profile: root.join("profile"),
        };
        fs::create_dir_all(&locations.state).unwrap();
        fs::create_dir_all(&locations.profile).unwrap();
        fs::write(locations.profile.join("Bookmarks"), b"synthetic bookmarks").unwrap();
        fake_app(&locations.app, 1);
        let staged = locations.state.join("staged/2/Stainless.app");
        fake_app(&staged, 2);
        let feed = UpdateFeed {
            schema_version: 1,
            product: "stainless".into(),
            release_tag: product(2).release_tag(),
            release: product(2),
            os: "macos".into(),
            arch: "arm64".into(),
            minimum_macos: "13.0".into(),
            chromium_revision: "a".repeat(40),
            chromium_version: "156.0.8000.1".into(),
            asset: Asset {
                name: "Stainless-0.1.0-preview.2-macos-arm64.zip".into(),
                size: 1,
                sha256: "a".repeat(64),
            },
        };
        write_json(&locations.state.join("ready.json"), &feed).unwrap();
        locations
    }

    #[test]
    fn reports_cached_staging_offline_even_after_a_failed_check() {
        let root = tempfile::tempdir().unwrap();
        let locations = fixture(root.path());
        write_json(
            &locations.state.join("status.json"),
            &Status::new("error", "synthetic offline response"),
        )
        .unwrap();
        let result = status(&locations).unwrap();
        assert_eq!(result.state, "staged");
        assert_eq!(result.version.as_deref(), Some("0.1.0-preview.2"));
        assert!(result.message.contains("synthetic offline response"));
    }

    #[test]
    fn pending_transaction_status_does_not_treat_its_own_version_as_a_downgrade() {
        let root = tempfile::tempdir().unwrap();
        let locations = fixture(root.path());
        install_with(&locations, fake_validation).unwrap();
        assert_eq!(status(&locations).unwrap().state, "awaiting_launch");
        reconcile(&locations, true).unwrap();
        assert_eq!(status(&locations).unwrap().state, "rolled_back");
    }

    #[test]
    fn a_verified_redownload_repairs_corrupt_staging_without_replacing_the_browser() {
        use std::io::Write;
        let root = tempfile::tempdir().unwrap();
        let locations = fixture(root.path());
        let staged = locations.state.join("staged/2/Stainless.app");
        let valid_info = fs::read(staged.join("Contents/Info.plist")).unwrap();
        fs::write(
            staged.join("Contents/Info.plist"),
            b"synthetic corrupt cache",
        )
        .unwrap();
        let archive = root.path().join("synthetic-update.zip");
        let mut writer = zip::ZipWriter::new(File::create(&archive).unwrap());
        writer
            .start_file(
                "Stainless.app/Contents/Info.plist",
                zip::write::SimpleFileOptions::default(),
            )
            .unwrap();
        writer.write_all(&valid_info).unwrap();
        writer.finish().unwrap();
        let mut feed: UpdateFeed =
            serde_json::from_slice(&fs::read(locations.state.join("ready.json")).unwrap()).unwrap();
        feed.asset.size = archive.metadata().unwrap().len();
        feed.asset.sha256 = stainless_release::sha256(&archive).unwrap();
        stage_with(&locations, &feed, &archive, fake_validation).unwrap();
        assert_eq!(bundle::product(&staged).unwrap(), product(2));
        assert_eq!(bundle::product(&locations.app).unwrap(), product(1));
        assert_eq!(
            fs::read(locations.profile.join("Bookmarks")).unwrap(),
            b"synthetic bookmarks"
        );
    }

    fn fake_validation(app: &Path, expected: &Product) -> Result<()> {
        ensure!(bundle::product(app)? == *expected, "Wrong fixture product");
        Ok(())
    }

    #[test]
    fn installs_without_losing_profile_or_previous_app() {
        let root = tempfile::tempdir().unwrap();
        let locations = fixture(root.path());
        install_with(&locations, fake_validation).unwrap();
        assert_eq!(bundle::product(&locations.app).unwrap(), product(2));
        assert_eq!(
            fs::read(locations.profile.join("Bookmarks")).unwrap(),
            b"synthetic bookmarks"
        );
        let transaction: Transaction =
            serde_json::from_slice(&fs::read(locations.state.join("transaction.json")).unwrap())
                .unwrap();
        assert_eq!(
            bundle::product(&transaction.previous()).unwrap(),
            product(1)
        );
        assert!(transaction.snapshot(&locations).join("Bookmarks").is_file());
    }

    #[test]
    fn refuses_replacement_while_a_browser_holds_its_shared_lock() {
        let root = tempfile::tempdir().unwrap();
        let locations = fixture(root.path());
        let lock = locations.lock("install.lock").unwrap();
        FileExt::lock_shared(&lock).unwrap();
        assert!(install_with(&locations, fake_validation).is_err());
        assert_eq!(bundle::product(&locations.app).unwrap(), product(1));
    }

    #[test]
    fn rolls_back_failed_launch_and_restores_profile_snapshot() {
        let root = tempfile::tempdir().unwrap();
        let locations = fixture(root.path());
        install_with(&locations, fake_validation).unwrap();
        fs::write(
            locations.profile.join("Bookmarks"),
            b"failed version changed profile",
        )
        .unwrap();
        assert_eq!(reconcile(&locations, true).unwrap().state, "rolled_back");
        assert_eq!(bundle::product(&locations.app).unwrap(), product(1));
        assert_eq!(
            fs::read(locations.profile.join("Bookmarks")).unwrap(),
            b"synthetic bookmarks"
        );
    }

    #[test]
    fn does_not_rollback_before_a_first_launch() {
        let root = tempfile::tempdir().unwrap();
        let locations = fixture(root.path());
        install_with(&locations, fake_validation).unwrap();
        assert_eq!(
            reconcile(&locations, false).unwrap().state,
            "awaiting_launch"
        );
        assert_eq!(bundle::product(&locations.app).unwrap(), product(2));
    }

    #[test]
    fn keeps_healthy_update_and_last_known_good_backup() {
        let root = tempfile::tempdir().unwrap();
        let locations = fixture(root.path());
        install_with(&locations, fake_validation).unwrap();
        fs::create_dir_all(locations.state.join("health")).unwrap();
        fs::write(locations.state.join("health/2"), "ready\n").unwrap();
        assert_eq!(reconcile(&locations, true).unwrap().state, "healthy");
        assert_eq!(bundle::product(&locations.app).unwrap(), product(2));
        assert!(!locations.state.join("ready.json").exists());
    }

    #[test]
    fn recovers_power_interruption_between_swap_and_journal_update() {
        let root = tempfile::tempdir().unwrap();
        let locations = fixture(root.path());
        install_with(&locations, fake_validation).unwrap();
        let path = locations.state.join("transaction.json");
        let mut transaction: Transaction =
            serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        fs::rename(transaction.previous(), transaction.incoming()).unwrap();
        transaction.phase = Phase::Prepared;
        write_json(&path, &transaction).unwrap();
        assert_eq!(reconcile(&locations, true).unwrap().state, "rolled_back");
        assert_eq!(bundle::product(&locations.app).unwrap(), product(1));
    }

    #[test]
    fn rejects_tampered_recovery_paths_without_touching_app() {
        let root = tempfile::tempdir().unwrap();
        let locations = fixture(root.path());
        install_with(&locations, fake_validation).unwrap();
        let path = locations.state.join("transaction.json");
        let mut transaction: Transaction =
            serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        transaction.id = "../outside".into();
        write_json(&path, &transaction).unwrap();
        assert!(reconcile(&locations, true).is_err());
        assert_eq!(bundle::product(&locations.app).unwrap(), product(2));
    }

    #[test]
    fn does_not_retry_a_failed_launch_unattended() {
        let root = tempfile::tempdir().unwrap();
        let locations = fixture(root.path());
        install_with(&locations, fake_validation).unwrap();
        reconcile(&locations, true).unwrap();
        assert!(reject_failed_build(&locations, 2).is_err());
        reject_failed_build(&locations, 3).unwrap();
        assert!(!locations.state.join("ready.json").exists());
    }

    #[test]
    fn denied_installation_permission_preserves_original_app_and_profile() {
        use std::os::unix::fs::PermissionsExt;
        let root = tempfile::tempdir().unwrap();
        let locations = fixture(root.path());
        let parent = locations.app.parent().unwrap();
        fs::set_permissions(parent, fs::Permissions::from_mode(0o555)).unwrap();
        let result = install_with(&locations, fake_validation);
        fs::set_permissions(parent, fs::Permissions::from_mode(0o755)).unwrap();
        assert!(result.is_err());
        assert_eq!(bundle::product(&locations.app).unwrap(), product(1));
        assert_eq!(
            fs::read(locations.profile.join("Bookmarks")).unwrap(),
            b"synthetic bookmarks"
        );
    }
}
