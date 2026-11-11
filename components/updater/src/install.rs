use crate::{Locations, Status, bundle, write_json};
use anyhow::{Context, Result, ensure};
use fs2::FileExt;
use serde::{Deserialize, Serialize};
use stainless_profiles::{app, files::swap, updates::UpdateProfiles};
use stainless_release::{Product, UpdateFeed, verify_asset};
use std::fs::{self, File};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Transaction {
    #[serde(default = "legacy_transaction_version")]
    schema_version: u32,
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
    RollingBack,
    Healthy,
    RolledBack,
}

fn legacy_transaction_version() -> u32 {
    1
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
    fn snapshot_parent(&self, locations: &Locations) -> PathBuf {
        locations.state.join("backups").join(&self.id)
    }
    fn validate(&self, locations: &Locations) -> Result<()> {
        ensure!(
            self.app == locations.app,
            "Recovery belongs to a different application"
        );
        ensure!(
            matches!(self.schema_version, 1 | 2)
                && (self.phase != Phase::RollingBack || self.schema_version == 2),
            "Unsupported update transaction version; preserve its recovery state"
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
        && matches!(
            transaction.phase,
            Phase::Prepared | Phase::Installed | Phase::RollingBack
        )
    {
        return Ok(Status::for_release(
            if transaction.phase == Phase::RollingBack {
                "recovering"
            } else {
                "awaiting_launch"
            },
            "An update transaction is awaiting startup confirmation or complete profile/application recovery.",
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
    let profiles = UpdateProfiles::lock(&locations.profile, lock)?;
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
    let current = app::browser(&locations.app)?;
    feed.validate_upgrade(&current.product, &os_version()?)?;
    let staged = locations
        .state
        .join("staged")
        .join(feed.release.build.to_string())
        .join("Stainless.app");
    validate(&staged, &feed.release)?;
    let next = app::browser(&staged)?;
    let id = SystemTime::now()
        .duration_since(UNIX_EPOCH)?
        .as_nanos()
        .to_string();
    let mut transaction = Transaction {
        schema_version: 2,
        id,
        app: locations.app.clone(),
        old: current.product.clone(),
        new: feed.release.clone(),
        phase: Phase::Prepared,
        observed_launch: false,
        had_profile: false,
    };
    let snapshot_parent = transaction.snapshot_parent(locations);
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
    fs::set_permissions(&snapshot_parent, fs::Permissions::from_mode(0o700))?;
    transaction.had_profile =
        profiles.capture(&snapshot_parent, &transaction.id, &current, &next)?;
    File::open(snapshot_parent.parent().context("Missing backup parent")?)?.sync_all()?;
    let health = locations
        .state
        .join("health")
        .join(transaction.new.build.to_string());
    if health.exists() {
        fs::remove_file(health)?;
    }
    // A rename may commit even when the following directory sync reports failure.
    preparation.journaled = true;
    write_json(&transaction_path, &transaction)?;
    swap(&locations.app, &transaction.incoming())?;
    File::open(locations.app.parent().context("Missing app parent")?)?.sync_all()?;
    fs::rename(transaction.incoming(), transaction.previous())?;
    File::open(locations.app.parent().context("Missing app parent")?)?.sync_all()?;
    let transaction = Transaction {
        phase: Phase::Installed,
        ..transaction
    };
    write_json(&transaction_path, &transaction)?;
    Ok(Status::new(
        "awaiting_launch",
        "Update installed. The previous app, preview and every registered managed-profile snapshot are retained until a healthy launch is confirmed.",
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
    if transaction.schema_version == 2 {
        ensure!(
            UpdateProfiles::check_snapshot(
                &transaction.snapshot_parent(locations),
                &transaction.id,
                &transaction.old,
                &transaction.new,
            )? == transaction.had_profile,
            "Profile snapshot omits or changes the preview ownership recorded in its transaction"
        );
    }
    let installed = bundle::product(&locations.app)?;
    if installed == transaction.old && transaction.phase != Phase::RollingBack {
        ensure!(
            transaction.phase == Phase::Prepared || transaction.schema_version == 1,
            "The application was downgraded outside its recovery transaction; profile recovery is still required"
        );
        if transaction.schema_version == 1 && transaction.phase != Phase::Prepared {
            let profiles =
                UpdateProfiles::lock(&locations.profile, locations.lock("install.lock")?)?;
            profiles.check_legacy_rollback()?;
        }
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
        installed == transaction.new
            || (installed == transaction.old && transaction.phase == Phase::RollingBack),
        "Unexpected installed version; automatic recovery stopped"
    );
    if observed_start && transaction.phase != Phase::RollingBack {
        transaction.observed_launch = true;
        write_json(&path, &transaction)?;
    }
    let health = locations
        .state
        .join("health")
        .join(transaction.new.build.to_string());
    if transaction.phase != Phase::RollingBack && health.is_file() {
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
            File::open(locations.app.parent().context("Missing app parent")?)?.sync_all()?;
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
            return Ok(match transaction.phase {
                Phase::RollingBack => Status::new(
                    "recovering",
                    "Profile/application rollback is unfinished. Quit every Stainless browser to resume recovery; a startup signal cannot cancel it.",
                ),
                _ => Status::new(
                    "awaiting_health",
                    "Waiting for browser startup confirmation. If startup hangs, quit Stainless to recover the retained previous app/profile.",
                ),
            });
        }
        return Err(error).context("Cannot acquire the installation recovery lock");
    }
    if !transaction.observed_launch && transaction.phase != Phase::RollingBack {
        return Ok(Status::new(
            "awaiting_launch",
            "Update installed; waiting for its first launch.",
        ));
    }
    let profiles = UpdateProfiles::lock(&locations.profile, lock)?;
    let previous = if transaction.previous().exists() {
        transaction.previous()
    } else {
        transaction.incoming()
    };
    if installed == transaction.new {
        ensure!(
            bundle::product(&previous)? == transaction.old,
            "Previous app does not match the recovery journal; no files were replaced"
        );
    }
    if transaction.schema_version == 1 {
        let old = app::browser(&previous)?;
        let new = app::browser(&locations.app)?;
        profiles.adopt_legacy(
            &transaction.snapshot_parent(locations),
            &transaction.id,
            &old,
            &new,
            transaction.had_profile,
        )?;
    }
    profiles.prepare_rollback(
        &transaction.snapshot_parent(locations),
        &transaction.id,
        &transaction.old,
        &transaction.new,
    )?;
    // Keep the new app installed until every root and its compatibility floor is restored.
    transaction.schema_version = 2;
    transaction.phase = Phase::RollingBack;
    write_json(&path, &transaction)?;
    profiles.restore(
        &transaction.snapshot_parent(locations),
        &transaction.id,
        &transaction.old,
        &transaction.new,
    )?;
    if installed == transaction.new {
        swap(&locations.app, &previous)?;
        File::open(locations.app.parent().context("Missing app parent")?)?.sync_all()?;
    }
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
            "New browser did not complete startup. Previous app, preview, managed profiles and compatibility metadata restored; failed state retained for inspection.",
            &transaction.old,
        ),
    )
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
    use stainless_profiles::{BrowserIdentity, Store};
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
        use std::os::unix::fs::PermissionsExt;

        fs::create_dir_all(app.join("Contents/MacOS")).unwrap();
        let data = serde_json::json!({
            "CFBundleIdentifier": "dev.stainless.browser",
            "CFBundleExecutable": "Stainless",
            "CFBundleShortVersionString": format!("156.0.8000.{build}"),
            "CrProductDirName": "Stainless",
            "StainlessVersion": product(build).version.to_string(),
            "StainlessBuild": build,
            "StainlessChannel": "preview",
            "StainlessProfileSchema": 1,
        });
        plist::to_file_xml(app.join("Contents/Info.plist"), &data).unwrap();
        let executable = app.join("Contents/MacOS/Stainless");
        fs::write(
            &executable,
            format!("#!/bin/sh\nset -eu\nprintf '156.0.8000.{build}' > \"${{1#--user-data-dir=}}/Last Version\"\n"),
        )
        .unwrap();
        fs::set_permissions(executable, fs::Permissions::from_mode(0o700)).unwrap();
    }

    fn fixture(root: &Path) -> Locations {
        let support = root.join("Library/Application Support");
        let locations = Locations {
            app: root.join("Applications/Stainless.app"),
            state: support.join("Stainless Updater"),
            profile: support.join("Stainless"),
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

    fn managed_root(store: &Store, locations: &Locations, label: &str) -> PathBuf {
        let profile = store.create(label).unwrap();
        store.run(&profile.id, &locations.app).unwrap();
        let directory = locations
            .profile
            .with_file_name("Stainless Profiles")
            .join("profiles")
            .join(profile.id.to_string());
        fs::create_dir(directory.join("data/Default")).unwrap();
        fs::write(directory.join("data/Default/Bookmarks"), label).unwrap();
        directory
    }

    fn journal(locations: &Locations) -> Transaction {
        serde_json::from_slice(&fs::read(locations.state.join("transaction.json")).unwrap())
            .unwrap()
    }

    fn begin_recovery(locations: &Locations) -> Transaction {
        let mut transaction = journal(locations);
        let profiles =
            UpdateProfiles::lock(&locations.profile, locations.lock("install.lock").unwrap())
                .unwrap();
        profiles
            .prepare_rollback(
                &transaction.snapshot_parent(locations),
                &transaction.id,
                &transaction.old,
                &transaction.new,
            )
            .unwrap();
        transaction.phase = Phase::RollingBack;
        transaction.observed_launch = true;
        write_json(&locations.state.join("transaction.json"), &transaction).unwrap();
        transaction
    }

    fn advance_managed_registry(locations: &Locations) {
        let registry_path = locations
            .profile
            .with_file_name("Stainless Profiles")
            .join("registry.json");
        let mut registry: serde_json::Value =
            serde_json::from_slice(&fs::read(&registry_path).unwrap()).unwrap();
        for profile in registry["profiles"].as_array_mut().unwrap() {
            profile["last_browser"] = serde_json::to_value(BrowserIdentity {
                product: product(2),
                chromium_version: "156.0.8000.2".into(),
            })
            .unwrap();
        }
        write_json(&registry_path, &registry).unwrap();
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
        assert!(
            transaction
                .snapshot_parent(&locations)
                .join("profile/Bookmarks")
                .is_file()
        );
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
    fn rolls_back_every_registered_root_and_its_browser_compatibility_floor() {
        let root = tempfile::tempdir().unwrap();
        let locations = fixture(root.path());
        let store = Store::for_home(root.path()).unwrap();
        let mut managed = Vec::new();
        for label in ["Synthetic work", "Synthetic personal"] {
            let directory = managed_root(&store, &locations, label);
            managed.push((directory, label));
        }
        let before = store.list().unwrap();
        install_with(&locations, fake_validation).unwrap();
        fs::write(
            locations.profile.join("Bookmarks"),
            b"synthetic failed preview",
        )
        .unwrap();
        for (directory, _) in &managed {
            fs::write(
                directory.join("data/Default/Bookmarks"),
                b"synthetic failed managed state",
            )
            .unwrap();
            fs::write(directory.join("data/Last Version"), b"156.0.8000.2").unwrap();
        }
        advance_managed_registry(&locations);

        assert_eq!(reconcile(&locations, true).unwrap().state, "rolled_back");
        assert_eq!(bundle::product(&locations.app).unwrap(), product(1));
        assert_eq!(
            fs::read(locations.profile.join("Bookmarks")).unwrap(),
            b"synthetic bookmarks"
        );
        let transaction: Transaction =
            serde_json::from_slice(&fs::read(locations.state.join("transaction.json")).unwrap())
                .unwrap();
        for (directory, label) in &managed {
            assert_eq!(
                fs::read(directory.join("data/Default/Bookmarks")).unwrap(),
                label.as_bytes()
            );
            assert_eq!(
                fs::read(directory.join("data/Last Version")).unwrap(),
                b"156.0.8000.1"
            );
            assert_eq!(
                fs::read(
                    directory
                        .join(format!("failed-{}", transaction.id))
                        .join("Default/Bookmarks")
                )
                .unwrap(),
                b"synthetic failed managed state"
            );
        }
        let recovery: serde_json::Value = serde_json::from_slice(
            &fs::read(
                transaction
                    .snapshot_parent(&locations)
                    .join("recovery.json"),
            )
            .unwrap(),
        )
        .unwrap();
        assert_eq!(
            recovery["failed_registry"]["profiles"][0]["last_browser"]["product"]["build"],
            2
        );
        store.run(&before[0].id, &locations.app).unwrap();
        assert_eq!(store.list().unwrap(), before);
    }

    #[test]
    fn resumes_interrupted_multi_root_recovery_without_accepting_late_health() {
        for interruption in 0..4 {
            let root = tempfile::tempdir().unwrap();
            let locations = fixture(root.path());
            let store = Store::for_home(root.path()).unwrap();
            let first = managed_root(&store, &locations, "Synthetic work");
            let second = managed_root(&store, &locations, "Synthetic personal");
            let before = store.list().unwrap();
            install_with(&locations, fake_validation).unwrap();
            fs::write(
                locations.profile.join("Bookmarks"),
                b"synthetic failed preview",
            )
            .unwrap();
            for managed in [&first, &second] {
                fs::write(
                    managed.join("data/Default/Bookmarks"),
                    b"synthetic failed managed",
                )
                .unwrap();
                fs::write(managed.join("data/Last Version"), b"156.0.8000.2").unwrap();
            }
            advance_managed_registry(&locations);
            let transaction = begin_recovery(&locations);
            let plan: serde_json::Value = serde_json::from_slice(
                &fs::read(
                    transaction
                        .snapshot_parent(&locations)
                        .join("recovery.json"),
                )
                .unwrap(),
            )
            .unwrap();
            if interruption < 2 {
                let staged = locations
                    .profile
                    .parent()
                    .unwrap()
                    .join(plan["roots"][0]["staging"].as_str().unwrap())
                    .join("data");
                swap(&locations.profile, &staged).unwrap();
                if interruption == 1 {
                    fs::rename(
                        &staged,
                        locations.profile.with_file_name(format!(
                            ".Stainless.failed-profile-{}",
                            transaction.id
                        )),
                    )
                    .unwrap();
                    let staged = first
                        .join(plan["roots"][1]["staging"].as_str().unwrap())
                        .join("data");
                    swap(&first.join("data"), &staged).unwrap();
                }
            } else {
                let profiles = UpdateProfiles::lock(
                    &locations.profile,
                    locations.lock("install.lock").unwrap(),
                )
                .unwrap();
                profiles
                    .restore(
                        &transaction.snapshot_parent(&locations),
                        &transaction.id,
                        &transaction.old,
                        &transaction.new,
                    )
                    .unwrap();
                if interruption == 3 {
                    swap(&locations.app, &transaction.previous()).unwrap();
                }
            }
            fs::create_dir_all(locations.state.join("health")).unwrap();
            fs::write(locations.state.join("health/2"), "ready\n").unwrap();
            assert_eq!(status(&locations).unwrap().state, "recovering");
            assert_eq!(reconcile(&locations, false).unwrap().state, "rolled_back");
            assert_eq!(bundle::product(&locations.app).unwrap(), product(1));
            assert_eq!(store.list().unwrap(), before);
            for (managed, expected) in [(&first, "Synthetic work"), (&second, "Synthetic personal")]
            {
                assert_eq!(
                    fs::read(managed.join("data/Default/Bookmarks")).unwrap(),
                    expected.as_bytes(),
                );
                assert_eq!(
                    fs::read(managed.join(format!("failed-{}/Default/Bookmarks", transaction.id)))
                        .unwrap(),
                    b"synthetic failed managed",
                );
            }
            assert_eq!(
                fs::read(
                    locations
                        .profile
                        .with_file_name(format!(".Stainless.failed-profile-{}", transaction.id))
                        .join("Bookmarks")
                )
                .unwrap(),
                b"synthetic failed preview",
            );
            assert_eq!(reconcile(&locations, false).unwrap().state, "rolled_back");
        }
    }

    #[test]
    fn a_partial_profile_restore_blocks_app_rollback_and_can_resume_after_permission_repair() {
        let root = tempfile::tempdir().unwrap();
        let locations = fixture(root.path());
        let store = Store::for_home(root.path()).unwrap();
        let first = managed_root(&store, &locations, "Synthetic work");
        let second = managed_root(&store, &locations, "Synthetic personal");
        let before = store.list().unwrap();
        install_with(&locations, fake_validation).unwrap();
        fs::write(
            locations.profile.join("Bookmarks"),
            b"synthetic failed preview",
        )
        .unwrap();
        for directory in [&first, &second] {
            fs::write(
                directory.join("data/Default/Bookmarks"),
                b"synthetic failed managed",
            )
            .unwrap();
        }
        begin_recovery(&locations);
        fs::set_permissions(&second, fs::Permissions::from_mode(0o500)).unwrap();
        let result = reconcile(&locations, false);
        fs::set_permissions(&second, fs::Permissions::from_mode(0o700)).unwrap();
        assert!(result.is_err());
        assert_eq!(journal(&locations).phase, Phase::RollingBack);
        assert_eq!(status(&locations).unwrap().state, "recovering");
        assert_eq!(bundle::product(&locations.app).unwrap(), product(2));
        assert_eq!(
            fs::read(first.join("data/Default/Bookmarks")).unwrap(),
            b"Synthetic work",
        );
        assert_eq!(
            fs::read(second.join("data/Default/Bookmarks")).unwrap(),
            b"synthetic failed managed",
        );
        let browser = locations.lock("install.lock").unwrap();
        FileExt::lock_shared(&browser).unwrap();
        assert_eq!(reconcile(&locations, false).unwrap().state, "recovering");
        drop(browser);
        assert_eq!(reconcile(&locations, false).unwrap().state, "rolled_back");
        assert_eq!(bundle::product(&locations.app).unwrap(), product(1));
        assert_eq!(store.list().unwrap(), before);
        assert_eq!(
            fs::read(second.join("data/Default/Bookmarks")).unwrap(),
            b"Synthetic personal",
        );
    }

    #[test]
    fn incomplete_recovery_plan_cannot_drop_a_managed_root_or_commit_a_rollback() {
        let root = tempfile::tempdir().unwrap();
        let locations = fixture(root.path());
        let store = Store::for_home(root.path()).unwrap();
        let managed = managed_root(&store, &locations, "Synthetic work");
        install_with(&locations, fake_validation).unwrap();
        fs::write(
            locations.profile.join("Bookmarks"),
            b"synthetic failed preview",
        )
        .unwrap();
        let transaction = begin_recovery(&locations);
        let path = transaction
            .snapshot_parent(&locations)
            .join("recovery.json");
        let mut plan: serde_json::Value =
            serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        plan["roots"][1]["restored"] = serde_json::Value::Null;
        plan["roots"][1]["staging"] = serde_json::Value::Null;
        write_json(&path, &plan).unwrap();

        assert!(reconcile(&locations, false).is_err());
        assert_eq!(journal(&locations).phase, Phase::RollingBack);
        assert_eq!(bundle::product(&locations.app).unwrap(), product(2));
        assert_eq!(
            fs::read(locations.profile.join("Bookmarks")).unwrap(),
            b"synthetic failed preview"
        );
        assert_eq!(
            fs::read(managed.join("data/Default/Bookmarks")).unwrap(),
            b"Synthetic work"
        );
    }

    #[test]
    fn legacy_journals_recover_preview_only_installs_but_cannot_omit_managed_roots() {
        for with_managed in [false, true] {
            let root = tempfile::tempdir().unwrap();
            let locations = fixture(root.path());
            let managed = if with_managed {
                let store = Store::for_home(root.path()).unwrap();
                Some(managed_root(&store, &locations, "Synthetic work"))
            } else {
                None
            };
            install_with(&locations, fake_validation).unwrap();
            let transaction = journal(&locations);
            let path = locations.state.join("transaction.json");
            let mut legacy = serde_json::to_value(&transaction).unwrap();
            legacy.as_object_mut().unwrap().remove("schema_version");
            write_json(&path, &legacy).unwrap();
            fs::remove_file(
                transaction
                    .snapshot_parent(&locations)
                    .join("profiles.json"),
            )
            .unwrap();
            fs::write(
                locations.profile.join("Bookmarks"),
                b"synthetic failed preview",
            )
            .unwrap();

            let result = reconcile(&locations, true);
            if let Some(managed) = managed {
                assert!(result.unwrap_err().to_string().contains("Legacy update"));
                assert_eq!(bundle::product(&locations.app).unwrap(), product(2));
                assert_eq!(journal(&locations).phase, Phase::Installed);
                assert_eq!(
                    fs::read(locations.profile.join("Bookmarks")).unwrap(),
                    b"synthetic failed preview",
                );
                assert_eq!(
                    fs::read(managed.join("data/Default/Bookmarks")).unwrap(),
                    b"Synthetic work",
                );
                swap(&locations.app, &transaction.previous()).unwrap();
                assert!(reconcile(&locations, false).is_err());
                assert_eq!(journal(&locations).phase, Phase::Installed);
            } else {
                assert_eq!(result.unwrap().state, "rolled_back");
                assert_eq!(journal(&locations).schema_version, 2);
                assert_eq!(bundle::product(&locations.app).unwrap(), product(1));
                assert_eq!(
                    fs::read(locations.profile.join("Bookmarks")).unwrap(),
                    b"synthetic bookmarks",
                );
                assert!(
                    !locations
                        .profile
                        .with_file_name("Stainless Profiles")
                        .exists()
                );
            }
        }
    }

    #[test]
    fn unsafe_or_unpaired_managed_roots_block_the_whole_install_without_mutation() {
        for failure in [
            "future_registry",
            "missing_registry",
            "owned_root",
            "newer_browser",
            "symlink",
        ] {
            let root = tempfile::tempdir().unwrap();
            let locations = fixture(root.path());
            let store = Store::for_home(root.path()).unwrap();
            let managed = managed_root(&store, &locations, "Synthetic work");
            let registry_path = managed
                .parent()
                .unwrap()
                .parent()
                .unwrap()
                .join("registry.json");
            let mut registry: serde_json::Value =
                serde_json::from_slice(&fs::read(&registry_path).unwrap()).unwrap();
            let owner = if failure == "owned_root" {
                let owner =
                    stainless_profiles::files::open_lock(&managed.join("owner.lock")).unwrap();
                owner.try_lock_exclusive().unwrap();
                Some(owner)
            } else {
                None
            };
            match failure {
                "future_registry" => {
                    registry["schema_version"] = 2.into();
                    write_json(&registry_path, &registry).unwrap();
                }
                "missing_registry" => fs::remove_file(&registry_path).unwrap(),
                "newer_browser" => {
                    registry["profiles"][0]["last_browser"] =
                        serde_json::to_value(BrowserIdentity {
                            product: product(3),
                            chromium_version: "156.0.8000.3".into(),
                        })
                        .unwrap();
                    write_json(&registry_path, &registry).unwrap();
                    fs::write(managed.join("data/Last Version"), b"156.0.8000.3").unwrap();
                }
                "symlink" => std::os::unix::fs::symlink(
                    locations.profile.join("Bookmarks"),
                    managed.join("data/unexpected-link"),
                )
                .unwrap(),
                _ => {}
            }
            let registry_before = fs::read(&registry_path).ok();
            assert!(
                install_with(&locations, fake_validation).is_err(),
                "{failure}"
            );
            assert_eq!(bundle::product(&locations.app).unwrap(), product(1));
            assert!(!locations.state.join("transaction.json").exists());
            assert_eq!(fs::read(&registry_path).ok(), registry_before);
            assert_eq!(
                fs::read(locations.profile.join("Bookmarks")).unwrap(),
                b"synthetic bookmarks"
            );
            assert_eq!(
                fs::read(managed.join("data/Default/Bookmarks")).unwrap(),
                b"Synthetic work"
            );
            drop(owner);
        }
    }

    #[test]
    fn changed_registry_or_snapshot_blocks_recovery_before_any_live_root_is_replaced() {
        for failure in ["added_profile", "corrupt_snapshot", "future_snapshot"] {
            let root = tempfile::tempdir().unwrap();
            let locations = fixture(root.path());
            let store = Store::for_home(root.path()).unwrap();
            let managed = managed_root(&store, &locations, "Synthetic work");
            install_with(&locations, fake_validation).unwrap();
            let transaction = journal(&locations);
            fs::write(
                locations.profile.join("Bookmarks"),
                b"synthetic failed preview",
            )
            .unwrap();
            fs::write(
                managed.join("data/Default/Bookmarks"),
                b"synthetic failed managed",
            )
            .unwrap();
            let registry_path = managed
                .parent()
                .unwrap()
                .parent()
                .unwrap()
                .join("registry.json");
            match failure {
                "added_profile" => {
                    assert!(store.create("Blocked while pending").is_err());
                    let mut registry: serde_json::Value =
                        serde_json::from_slice(&fs::read(&registry_path).unwrap()).unwrap();
                    registry["profiles"].as_array_mut().unwrap().push(serde_json::json!({
                        "id": "p-abcdefghijklmnop", "label": "Synthetic external writer", "profile_schema": 1,
                    }));
                    fs::create_dir_all(managed.parent().unwrap().join("p-abcdefghijklmnop/data"))
                        .unwrap();
                    write_json(&registry_path, &registry).unwrap();
                }
                "corrupt_snapshot" => fs::write(
                    transaction
                        .snapshot_parent(&locations)
                        .join("profiles")
                        .join(managed.file_name().unwrap())
                        .join("Default/Bookmarks"),
                    b"synthetic corrupted snapshot",
                )
                .unwrap(),
                "future_snapshot" => {
                    let path = transaction
                        .snapshot_parent(&locations)
                        .join("profiles.json");
                    let mut snapshot: serde_json::Value =
                        serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
                    snapshot["schema_version"] = 2.into();
                    write_json(&path, &snapshot).unwrap();
                }
                _ => unreachable!(),
            }
            let registry_before = fs::read(&registry_path).unwrap();
            assert!(reconcile(&locations, true).is_err(), "{failure}");
            assert_eq!(journal(&locations).phase, Phase::Installed);
            assert_eq!(bundle::product(&locations.app).unwrap(), product(2));
            assert_eq!(fs::read(registry_path).unwrap(), registry_before);
            assert_eq!(
                fs::read(locations.profile.join("Bookmarks")).unwrap(),
                b"synthetic failed preview"
            );
            assert_eq!(
                fs::read(managed.join("data/Default/Bookmarks")).unwrap(),
                b"synthetic failed managed"
            );
        }
    }

    #[test]
    fn interrupted_recovery_retains_new_preview_data_when_only_managed_roots_existed_before_update()
    {
        let root = tempfile::tempdir().unwrap();
        let locations = fixture(root.path());
        fs::remove_file(locations.profile.join("Bookmarks")).unwrap();
        fs::remove_dir(&locations.profile).unwrap();
        let store = Store::for_home(root.path()).unwrap();
        let managed = managed_root(&store, &locations, "Synthetic work");
        let before = store.list().unwrap();
        install_with(&locations, fake_validation).unwrap();
        assert!(!journal(&locations).had_profile);
        fs::create_dir(&locations.profile).unwrap();
        fs::write(
            locations.profile.join("Bookmarks"),
            b"synthetic newly created preview",
        )
        .unwrap();
        let transaction = begin_recovery(&locations);
        let failed = locations
            .profile
            .with_file_name(format!(".Stainless.failed-profile-{}", transaction.id));
        fs::rename(&locations.profile, &failed).unwrap();
        assert_eq!(reconcile(&locations, false).unwrap().state, "rolled_back");
        assert_eq!(bundle::product(&locations.app).unwrap(), product(1));
        assert!(!locations.profile.exists());
        assert_eq!(
            fs::read(failed.join("Bookmarks")).unwrap(),
            b"synthetic newly created preview"
        );
        assert_eq!(
            fs::read(managed.join("data/Default/Bookmarks")).unwrap(),
            b"Synthetic work"
        );
        assert_eq!(store.list().unwrap(), before);
    }

    #[test]
    fn a_missing_preview_inventory_cannot_turn_existing_data_into_an_empty_profile_rollback() {
        let root = tempfile::tempdir().unwrap();
        let locations = fixture(root.path());
        install_with(&locations, fake_validation).unwrap();
        let transaction = journal(&locations);
        assert!(transaction.had_profile);
        let path = transaction
            .snapshot_parent(&locations)
            .join("profiles.json");
        let mut snapshot: serde_json::Value =
            serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        snapshot["preview"] = serde_json::Value::Null;
        write_json(&path, &snapshot).unwrap();
        fs::write(
            locations.profile.join("Bookmarks"),
            b"synthetic failed preview",
        )
        .unwrap();

        assert!(reconcile(&locations, true).is_err());
        assert_eq!(journal(&locations).phase, Phase::Installed);
        assert_eq!(bundle::product(&locations.app).unwrap(), product(2));
        assert_eq!(
            fs::read(locations.profile.join("Bookmarks")).unwrap(),
            b"synthetic failed preview"
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
        let store = Store::for_home(root.path()).unwrap();
        let managed = managed_root(&store, &locations, "Synthetic work");
        let profile = store.list().unwrap().remove(0);
        install_with(&locations, fake_validation).unwrap();
        assert!(store.run(&profile.id, &locations.app).is_err());
        assert!(store.create("Blocked while pending").is_err());
        assert_eq!(store.list().unwrap(), vec![profile.clone()]);
        let installation = locations.lock("install.lock").unwrap();
        FileExt::lock_shared(&installation).unwrap();
        fs::create_dir_all(locations.state.join("health")).unwrap();
        fs::write(locations.state.join("health/2"), "ready\n").unwrap();
        assert_eq!(reconcile(&locations, true).unwrap().state, "healthy");
        drop(installation);
        assert_eq!(bundle::product(&locations.app).unwrap(), product(2));
        assert!(!locations.state.join("ready.json").exists());
        let transaction = journal(&locations);
        assert_eq!(
            fs::read(
                transaction
                    .snapshot_parent(&locations)
                    .join("profiles")
                    .join(profile.id.to_string())
                    .join("Default/Bookmarks")
            )
            .unwrap(),
            b"Synthetic work",
        );
        store.run(&profile.id, &locations.app).unwrap();
        assert_eq!(
            store.list().unwrap()[0]
                .last_browser
                .as_ref()
                .unwrap()
                .product,
            product(2)
        );
        assert_eq!(
            fs::read(managed.join("data/Last Version")).unwrap(),
            b"156.0.8000.2"
        );
        assert_eq!(
            fs::read(managed.join("data/Default/Bookmarks")).unwrap(),
            b"Synthetic work"
        );
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

    #[test]
    fn retains_journaled_backups_when_the_final_directory_sync_fails() {
        let root = tempfile::tempdir().unwrap();
        let locations = fixture(root.path());
        let result = install_with(&locations, |app, expected| {
            fake_validation(app, expected)?;
            fs::set_permissions(&locations.state, fs::Permissions::from_mode(0o300))?;
            Ok(())
        });
        fs::set_permissions(&locations.state, fs::Permissions::from_mode(0o700)).unwrap();
        assert!(result.is_err());
        let transaction: Transaction =
            serde_json::from_slice(&fs::read(locations.state.join("transaction.json")).unwrap())
                .unwrap();
        assert_eq!(transaction.phase, Phase::Prepared);
        assert_eq!(bundle::product(&locations.app).unwrap(), product(1));
        assert_eq!(
            fs::read(
                transaction
                    .snapshot_parent(&locations)
                    .join("profile/Bookmarks")
            )
            .unwrap(),
            b"synthetic bookmarks"
        );
        assert!(
            transaction
                .snapshot_parent(&locations)
                .join("profiles.json")
                .is_file()
        );
        assert_eq!(
            bundle::product(&transaction.incoming()).unwrap(),
            product(2)
        );
        assert_eq!(reconcile(&locations, false).unwrap().state, "rolled_back");
    }
}
