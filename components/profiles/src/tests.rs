use super::*;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::time::{Duration, Instant};

#[test]
fn persists_only_profile_metadata_and_rejects_future_registries_without_rewriting_them() {
    let home = tempfile::tempdir().unwrap();
    let store = Store::for_home(home.path()).unwrap();
    let first = store.create("Synthetic work").unwrap();
    let second = store.create("Synthetic personal").unwrap();
    assert_ne!(first.id, second.id);
    assert_eq!(
        Store::for_home(home.path()).unwrap().list().unwrap(),
        vec![first, second]
    );
    let path = home
        .path()
        .join("Library/Application Support/Stainless Profiles/registry.json");
    let mut registry: serde_json::Value =
        serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    let profile = registry["profiles"][0].as_object().unwrap();
    assert_eq!(
        profile.keys().map(String::as_str).collect::<Vec<_>>(),
        ["id", "label", "profile_schema"]
    );
    registry["schema_version"] = 2.into();
    let future = serde_json::to_vec(&registry).unwrap();
    fs::write(&path, &future).unwrap();
    assert!(store.list().is_err());
    assert!(store.create("Must not appear").is_err());
    assert_eq!(fs::read(path).unwrap(), future);
}

pub(crate) fn browser(build: u64) -> BrowserIdentity {
    BrowserIdentity {
        product: stainless_release::Product {
            version: semver::Version::parse(&format!("0.1.0-preview.{build}")).unwrap(),
            build,
            channel: "preview".into(),
            profile_schema: PROFILE_SCHEMA,
        },
        chromium_version: format!("157.0.8103.{build}"),
    }
}

pub(crate) fn data(home: &Path, id: &ProfileId) -> PathBuf {
    home.join("Library/Application Support/Stainless Profiles/profiles")
        .join(id.to_string())
        .join("data")
}

pub(crate) fn synthetic_app(home: &Path, browser: &BrowserIdentity) -> PathBuf {
    let app = home
        .join(format!(
            "apps/{}-{}",
            browser.product.build, browser.product.profile_schema
        ))
        .join("Stainless.app");
    fs::create_dir_all(app.join("Contents/MacOS")).unwrap();
    let info = serde_json::json!({
        "CFBundleIdentifier": "dev.stainless.browser",
        "CFBundleExecutable": "Stainless",
        "CFBundleShortVersionString": browser.chromium_version,
        "CrProductDirName": "Stainless",
        "StainlessVersion": browser.product.version.to_string(),
        "StainlessBuild": browser.product.build,
        "StainlessChannel": browser.product.channel,
        "StainlessProfileSchema": browser.product.profile_schema,
    });
    plist::to_file_xml(app.join("Contents/Info.plist"), &info).unwrap();
    let executable = app.join("Contents/MacOS/Stainless");
    fs::write(&executable, b"#!/bin/sh\nexit 0\n").unwrap();
    fs::set_permissions(executable, fs::Permissions::from_mode(0o700)).unwrap();
    app
}

#[test]
fn owns_distinct_browser_processes_roots_and_caches_until_exit_and_blocks_downgrades() {
    let home = tempfile::tempdir().unwrap();
    let store = Store::for_home(home.path()).unwrap();
    let first = store.create("Synthetic first").unwrap();
    let second = store.create("Synthetic second").unwrap();
    let app = synthetic_app(home.path(), &browser(2));
    let executable = app.join("Contents/MacOS/Stainless");
    fs::write(
        &executable,
        b"#!/bin/sh\nset -eu\ndata=\"${1#--user-data-dir=}\"\ncache=\"${2#--disk-cache-dir=}\"\nprintf '%s' \"$data\" > \"$data/fixture-root\"\nprintf '%s' \"$cache\" > \"$data/fixture-cache\"\nprintf '%s' \"$$\" > \"$data/fixture-pid\"\ni=0\nwhile [ ! -f \"$data/stop\" ] && [ \"$i\" -lt 200 ]; do i=$((i + 1)); /bin/sleep 0.02; done\n",
    )
    .unwrap();
    fs::set_permissions(&executable, fs::Permissions::from_mode(0o700)).unwrap();
    let first_data = data(home.path(), &first.id);
    let second_data = data(home.path(), &second.id);
    std::thread::scope(|scope| {
        let first_run = scope.spawn(|| store.run(&first.id, &app).unwrap());
        let second_run = scope.spawn(|| store.run(&second.id, &app).unwrap());
        let deadline = Instant::now() + Duration::from_secs(5);
        while !first_data.join("fixture-pid").is_file()
            || !second_data.join("fixture-pid").is_file()
        {
            assert!(
                Instant::now() < deadline && !first_run.is_finished() && !second_run.is_finished(),
                "Both synthetic browser processes must start and stay owned"
            );
            std::thread::sleep(Duration::from_millis(10));
        }

        assert_ne!(
            fs::read(first_data.join("fixture-pid")).unwrap(),
            fs::read(second_data.join("fixture-pid")).unwrap()
        );
        for directory in [&first_data, &second_data] {
            assert_eq!(
                fs::read_to_string(directory.join("fixture-root")).unwrap(),
                fs::canonicalize(directory).unwrap().to_str().unwrap()
            );
            assert_eq!(
                fs::read_to_string(directory.join("fixture-cache")).unwrap(),
                fs::canonicalize(directory.parent().unwrap().join("cache"))
                    .unwrap()
                    .to_str()
                    .unwrap()
            );
        }
        assert!(store.run(&first.id, &app).is_err());
        let install = files::open_lock(
            &home
                .path()
                .join("Library/Application Support/Stainless Updater/install.lock"),
        )
        .unwrap();
        assert!(install.try_lock_exclusive().is_err());
        fs::write(first_data.join("stop"), b"").unwrap();
        fs::write(second_data.join("stop"), b"").unwrap();
        first_run.join().unwrap();
        second_run.join().unwrap();
        install.try_lock_exclusive().unwrap();
    });
    assert!(
        store
            .run(&first.id, &synthetic_app(home.path(), &browser(1)))
            .is_err()
    );
    assert_eq!(store.list().unwrap()[0].last_browser, Some(browser(2)));
}

#[test]
fn managed_incognito_transfers_only_an_explicit_url_and_discards_its_separate_root() {
    use crate::modes::{LaunchRequest, Mode};

    let home = tempfile::tempdir().unwrap();
    let store = Store::for_home(home.path()).unwrap();
    let profile = store.create("Synthetic identity").unwrap();
    let app = synthetic_app(home.path(), &browser(2));
    let info_path = app.join("Contents/Info.plist");
    let mut info = plist::Value::from_file(&info_path).unwrap();
    info.as_dictionary_mut()
        .unwrap()
        .insert("StainlessManagedModesVersion".into(), 1.into());
    plist::to_file_xml(&info_path, &info).unwrap();
    store.run(&profile.id, &app).unwrap();
    let persistent = data(home.path(), &profile.id);
    for name in ["Cookies", "Login Data", "History", "Bookmarks"] {
        fs::write(persistent.join(name), b"synthetic identity must stay here").unwrap();
    }
    let metadata = store.list().unwrap();
    let recording = home.path().join("arguments");
    fs::write(
        app.join("Contents/MacOS/Stainless"),
        format!(
            "#!/bin/sh\nset -eu\nprintf '%s\\n' \"$@\" > '{}'\ndata=\"${{1#--user-data-dir=}}\"\nfor name in Cookies 'Login Data' History Bookmarks; do test ! -e \"$data/$name\"; done\nprintf 'synthetic private state' > \"$data/History\"\n",
            recording.display()
        ),
    )
    .unwrap();
    let request = LaunchRequest::new(
        Mode::Incognito,
        Some("https://synthetic.invalid/explicit-transfer"),
    )
    .unwrap();
    let _persistent_owner = store.profile_lock(&profile.id).unwrap();
    let mut previous_root = None;
    for _ in 0..2 {
        store.launch(&profile.id, &app, &request).unwrap();
        let arguments = fs::read_to_string(&recording).unwrap();
        let arguments: Vec<_> = arguments.lines().collect();
        let private_root = PathBuf::from(arguments[0].strip_prefix("--user-data-dir=").unwrap());
        assert_ne!(private_root, persistent);
        assert!(!private_root.parent().unwrap().exists());
        assert_ne!(previous_root.as_ref(), Some(&private_root));
        previous_root = Some(private_root);
        assert_eq!(
            &arguments[2..],
            [
                "--stainless-managed-mode=incognito",
                "--incognito",
                "--disable-extensions",
                "--no-first-run",
                "--",
                "https://synthetic.invalid/explicit-transfer"
            ]
        );
    }
    assert_eq!(store.list().unwrap(), metadata);
    for name in ["Cookies", "Login Data", "History", "Bookmarks"] {
        assert_eq!(
            fs::read(persistent.join(name)).unwrap(),
            b"synthetic identity must stay here"
        );
    }
}

#[test]
fn unavailable_modes_and_implicit_credential_transfers_cannot_be_launched() {
    use crate::modes::{LaunchRequest, Mode};

    for mode in Mode::ALL {
        let result = LaunchRequest::new(mode, None);
        assert_eq!(result.is_ok(), mode.policy().unavailable_reason.is_none());
        if !matches!(mode, Mode::Standard | Mode::Incognito | Mode::Guest) {
            assert!(result.is_err(), "{mode:?} must remain unavailable");
        }
    }
    assert!("unknown-mode".parse::<Mode>().is_err());
    for url in [
        "https://user:password@synthetic.invalid/",
        "file:///synthetic/History",
        "javascript:alert(1)",
        "data:text/html,synthetic",
        "--no-sandbox",
        "https://synthetic.invalid/\n--no-sandbox",
    ] {
        for mode in [Mode::Standard, Mode::Incognito, Mode::Guest] {
            assert!(LaunchRequest::new(mode, Some(url)).is_err(), "{url}");
        }
    }
}

// The browser enforces the available rows in StainlessModePolicyMatrixBrowserTest
// (chromium/patches/0049-mode-policy-matrix-tests.patch). These invariants keep
// the matrix fail-closed for capabilities no mode implements yet.
#[test]
fn every_mode_denies_unimplemented_capabilities() {
    use crate::modes::{ExtensionPolicy, HistoryPolicy, Mode, StoragePolicy};

    for mode in Mode::ALL {
        let policy = mode.policy();
        assert!(!policy.ai && !policy.agents && !policy.vault, "{mode:?}");
        if policy.unavailable_reason.is_some() {
            assert!(matches!(policy.storage, StoragePolicy::Unavailable));
            assert!(matches!(policy.history, HistoryPolicy::Unavailable));
            assert!(matches!(policy.extensions, ExtensionPolicy::Unavailable));
            assert!(!policy.local_passwords && !policy.user_directed_downloads);
            assert!(!policy.direct_network, "{mode:?}");
        }
    }
}

#[test]
fn managed_incognito_refuses_older_apps_and_cleans_up_after_an_unsuccessful_child() {
    use crate::modes::{LaunchRequest, Mode};

    let home = tempfile::tempdir().unwrap();
    let store = Store::for_home(home.path()).unwrap();
    let profile = store.create("Synthetic").unwrap();
    let app = synthetic_app(home.path(), &browser(2));
    let request = LaunchRequest::new(Mode::Incognito, None).unwrap();
    let before = store.list().unwrap();
    let sessions = data(home.path(), &profile.id)
        .parent()
        .unwrap()
        .join("incognito");
    let error = store.launch(&profile.id, &app, &request).unwrap_err();
    assert!(error.to_string().contains("managed Incognito enforcement"));
    assert!(!sessions.exists());
    let info_path = app.join("Contents/Info.plist");
    let mut info = plist::Value::from_file(&info_path).unwrap();
    info.as_dictionary_mut()
        .unwrap()
        .insert("StainlessManagedModesVersion".into(), 1.into());
    plist::to_file_xml(&info_path, &info).unwrap();
    fs::write(app.join("Contents/MacOS/Stainless"), b"#!/bin/sh\nexit 7\n").unwrap();
    assert!(
        store
            .launch(&profile.id, &app, &request)
            .unwrap_err()
            .to_string()
            .contains("exited unsuccessfully")
    );
    assert!(fs::read_dir(sessions).unwrap().next().is_none());
    assert_eq!(store.list().unwrap(), before);
    store.profile_lock(&profile.id).unwrap();
    store.installation_lock(true).unwrap();
}

#[test]
fn metadata_protocol_cannot_request_paths_browsing_content_or_keys() {
    use crate::protocol::{MAX_REQUEST_BYTES, MetadataRequest};

    let home = tempfile::tempdir().unwrap();
    let store = Store::for_home(home.path()).unwrap();
    let created = store
        .metadata(
            MetadataRequest::parse(
                br#"{"protocol_version":1,"request":{"operation":"create","label":"Synthetic"}}"#,
            )
            .unwrap(),
        )
        .unwrap();
    let directory = data(home.path(), &created.profiles[0].id);
    fs::write(
        directory.join("History"),
        b"https://synthetic-private.invalid/",
    )
    .unwrap();
    let response = store
        .metadata(
            MetadataRequest::parse(br#"{"protocol_version":1,"request":{"operation":"list"}}"#)
                .unwrap(),
        )
        .unwrap();
    let serialized = serde_json::to_string(&response).unwrap();
    assert!(!serialized.contains("synthetic-private"));
    assert!(!serialized.contains(directory.to_str().unwrap()));
    assert_eq!(response.protocol_version, 1);
    assert_eq!(response.registry_version, 1);
    for request in [
        r#"{"protocol_version":2,"request":{"operation":"list"}}"#,
        r#"{"protocol_version":1,"request":{"operation":"list","path":"/tmp/History"}}"#,
        r#"{"protocol_version":1,"request":{"operation":"create","label":"X","key":"secret"}}"#,
        r#"{"protocol_version":1,"request":{"operation":"history"}}"#,
        r#"{"protocol_version":1,"request":{"operation":"launch","arguments":["--no-sandbox"]}}"#,
        r#"{"protocol_version":1,"request":{"operation":"list"},"vault":true}"#,
    ] {
        assert!(
            MetadataRequest::parse(request.as_bytes()).is_err(),
            "{request}"
        );
    }
    assert!(MetadataRequest::parse(&vec![b' '; MAX_REQUEST_BYTES + 1]).is_err());
    assert_eq!(store.list().unwrap().len(), 1);
}

#[test]
fn rejects_aliases_corrupt_registries_and_missing_registry_reinitialization() {
    use std::os::unix::fs::symlink;

    let home = tempfile::tempdir().unwrap();
    let store = Store::for_home(home.path()).unwrap();
    let profile = store.create("Synthetic").unwrap();
    let registry_path = home
        .path()
        .join("Library/Application Support/Stainless Profiles/registry.json");
    let original = fs::read(&registry_path).unwrap();
    let mut registry: serde_json::Value = serde_json::from_slice(&original).unwrap();
    let duplicate = registry["profiles"][0].clone();
    registry["profiles"].as_array_mut().unwrap().push(duplicate);
    fs::write(&registry_path, serde_json::to_vec(&registry).unwrap()).unwrap();
    assert!(store.list().is_err());
    registry["profiles"].as_array_mut().unwrap().pop();
    registry["profiles"][0]["history"] = "https://synthetic-private.invalid/".into();
    fs::write(&registry_path, serde_json::to_vec(&registry).unwrap()).unwrap();
    assert!(store.list().is_err());
    registry["profiles"][0]
        .as_object_mut()
        .unwrap()
        .remove("history");
    registry["profiles"][0]["id"] = "../outside".into();
    fs::write(&registry_path, serde_json::to_vec(&registry).unwrap()).unwrap();
    assert!(store.list().is_err());
    fs::write(&registry_path, &original).unwrap();
    let directory = data(home.path(), &profile.id);
    let retained = directory.with_file_name("retained-data");
    fs::rename(&directory, &retained).unwrap();
    symlink(&retained, &directory).unwrap();
    let app = synthetic_app(home.path(), &browser(2));
    assert!(store.run(&profile.id, &app).is_err());
    assert_eq!(fs::read(&registry_path).unwrap(), original);
    fs::remove_file(&directory).unwrap();
    fs::rename(&retained, &directory).unwrap();
    symlink(
        &registry_path,
        directory.parent().unwrap().join("owner.lock"),
    )
    .unwrap();
    assert!(store.run(&profile.id, &app).is_err());
    assert_eq!(fs::read(&registry_path).unwrap(), original);
    fs::remove_file(&registry_path).unwrap();
    assert!(Store::for_home(home.path()).is_err());
    assert!(!registry_path.exists());
    assert!(directory.is_dir());
}

fn guest_app(home: &Path) -> PathBuf {
    let app = synthetic_app(home, &browser(2));
    let info_path = app.join("Contents/Info.plist");
    let mut info = plist::Value::from_file(&info_path).unwrap();
    let dictionary = info.as_dictionary_mut().unwrap();
    dictionary.insert("StainlessManagedModesVersion".into(), 1.into());
    dictionary.insert("StainlessManagedGuestVersion".into(), 1.into());
    plist::to_file_xml(&info_path, &info).unwrap();
    app
}

fn guest_sessions(home: &Path) -> PathBuf {
    fs::canonicalize(home)
        .unwrap()
        .join("Library/Application Support/Stainless Profiles/guest")
}

fn guest_session_count(home: &Path) -> usize {
    fs::read_dir(guest_sessions(home))
        .unwrap()
        .filter(|entry| {
            entry
                .as_ref()
                .unwrap()
                .file_name()
                .to_string_lossy()
                .starts_with("g-")
        })
        .count()
}

fn device_is_attached(device: &str) -> bool {
    std::process::Command::new("/usr/sbin/diskutil")
        .args(["info", device])
        .output()
        .unwrap()
        .status
        .success()
}

#[test]
fn guest_keeps_its_whole_root_in_memory_and_erases_it_on_exit() {
    use crate::modes::{LaunchRequest, Mode};

    let home = tempfile::tempdir().unwrap();
    let store = Store::for_home(home.path()).unwrap();
    let profile = store.create("Synthetic identity").unwrap();
    let persistent = data(home.path(), &profile.id);
    fs::write(
        persistent.join("History"),
        b"synthetic identity must stay here",
    )
    .unwrap();
    let app = synthetic_app(home.path(), &browser(2));
    let request = LaunchRequest::new(Mode::Guest, Some("https://synthetic.invalid/guest")).unwrap();
    let error = store.launch_guest(&app, &request).unwrap_err();
    assert!(format!("{error:#}").contains("managed Incognito enforcement"));
    let app = guest_app(home.path());
    let older = app.join("Contents/Info.plist");
    let mut info = plist::Value::from_file(&older).unwrap();
    info.as_dictionary_mut()
        .unwrap()
        .remove("StainlessManagedGuestVersion");
    plist::to_file_xml(&older, &info).unwrap();
    let error = store.launch_guest(&app, &request).unwrap_err();
    assert!(format!("{error:#}").contains("memory-only Guest version 1"));
    assert!(!guest_sessions(home.path()).exists());
    let app = guest_app(home.path());
    let recording = home.path().join("guest-recording");
    fs::write(
        app.join("Contents/MacOS/Stainless"),
        format!(
            "#!/bin/sh\nset -eu\nprintf '%s\\n' \"$@\" > '{recording}'\ndata=\"${{1#--user-data-dir=}}\"\nprintf 'TMPDIR=%s\\n' \"$MAC_CHROMIUM_TMPDIR\" >> '{recording}'\ntest \"$TMPDIR\" = \"$MAC_CHROMIUM_TMPDIR\"\nprintf 'CRASHPAD=%s\\n' \"$BREAKPAD_DUMP_LOCATION\" >> '{recording}'\n/bin/df \"$data\" | /usr/bin/tail -1 | /usr/bin/awk '{{print \"DEVICE=\" $1}}' >> '{recording}'\ntest ! -e \"$data/History\"\nprintf 'synthetic guest state' > \"$data/History\"\nprintf 'synthetic temporary state' > \"$TMPDIR/upload\"\n",
            recording = recording.display()
        ),
    )
    .unwrap();
    let mut previous = None;
    for _ in 0..2 {
        store.launch_guest(&app, &request).unwrap();
        let lines: Vec<String> = fs::read_to_string(&recording)
            .unwrap()
            .lines()
            .map(str::to_owned)
            .collect();
        let root = PathBuf::from(lines[0].strip_prefix("--user-data-dir=").unwrap());
        assert!(root.starts_with(guest_sessions(home.path())));
        assert_ne!(previous.as_ref(), Some(&root));
        assert_eq!(
            &lines[2..8],
            [
                "--stainless-managed-mode=guest",
                "--incognito",
                "--disable-extensions",
                "--no-first-run",
                "--",
                "https://synthetic.invalid/guest"
            ]
        );
        let temporary = lines[8].strip_prefix("TMPDIR=").unwrap();
        assert!(Path::new(temporary).starts_with(root.parent().unwrap()));
        let crash_dumps = lines[9].strip_prefix("CRASHPAD=").unwrap();
        assert!(Path::new(crash_dumps).starts_with(root.parent().unwrap()));
        let device = lines[10].strip_prefix("DEVICE=").unwrap();
        assert!(device.starts_with("/dev/disk"), "{device}");
        assert!(!device_is_attached(device), "{device} must be ejected");
        assert!(!root.exists() && !Path::new(temporary).exists());
        assert_eq!(guest_session_count(home.path()), 0);
        previous = Some(root);
    }
    assert_eq!(
        fs::read(persistent.join("History")).unwrap(),
        b"synthetic identity must stay here"
    );
    assert!(store.launch(&profile.id, &app, &request).is_err());
    fs::write(app.join("Contents/MacOS/Stainless"), b"#!/bin/sh\nexit 7\n").unwrap();
    assert!(
        format!("{:#}", store.launch_guest(&app, &request).unwrap_err())
            .contains("exited unsuccessfully")
    );
    assert_eq!(guest_session_count(home.path()), 0);
    store.installation_lock(true).unwrap();
}

#[test]
fn guest_ejects_volumes_left_by_a_dead_supervisor_but_keeps_possibly_live_ones() {
    use crate::guest::RamVolume;
    use crate::modes::{LaunchRequest, Mode};

    let home = tempfile::tempdir().unwrap();
    let store = Store::for_home(home.path()).unwrap();
    let app = guest_app(home.path());
    let sessions = guest_sessions(home.path());
    files::directory(&sessions, true).unwrap();
    let stale = sessions.join("g-stale");
    let live = sessions.join("g-live");
    let mut volumes = Vec::new();
    for session in [&stale, &live] {
        fs::create_dir_all(session.join("volume")).unwrap();
        files::open_lock(&session.join("owner.lock")).unwrap();
        volumes.push(RamVolume::create(&session.join("volume"), 64 << 20).unwrap());
    }
    fs::create_dir(live.join("volume/data")).unwrap();
    std::os::unix::fs::symlink("synthetic-host-1", live.join("volume/data/SingletonLock")).unwrap();
    fs::create_dir(stale.join("volume/data")).unwrap();
    std::os::unix::fs::symlink(
        "synthetic-host-999999",
        stale.join("volume/data/SingletonLock"),
    )
    .unwrap();
    let orphan = sessions.join("g-orphan");
    fs::create_dir_all(orphan.join("volume/data")).unwrap();
    fs::write(
        orphan.join("volume/data/History"),
        b"synthetic on-disk guest state",
    )
    .unwrap();
    store
        .launch_guest(&app, &LaunchRequest::new(Mode::Guest, None).unwrap())
        .unwrap();
    assert!(
        !stale.exists(),
        "a dead browser's lock must not keep its volume"
    );
    assert!(
        !orphan.exists(),
        "lockless on-disk Guest roots must be removed"
    );
    volumes[1].verify().unwrap();
    volumes[1].eject().unwrap();
}

#[test]
fn guest_keeps_its_volume_while_a_relaunched_browser_still_uses_it() {
    use crate::modes::{LaunchRequest, Mode};

    let home = tempfile::tempdir().unwrap();
    let store = Store::for_home(home.path()).unwrap();
    let app = guest_app(home.path());
    let survivor = home.path().join("survivor-pid");
    fs::write(
        app.join("Contents/MacOS/Stainless"),
        format!(
            "#!/bin/sh\nset -eu\ndata=\"${{1#--user-data-dir=}}\"\n/bin/sleep 30 </dev/null >/dev/null 2>&1 &\nprintf '%s' \"$!\" > '{}'\nln -s \"synthetic-host-$!\" \"$data/SingletonLock\"\n",
            survivor.display()
        ),
    )
    .unwrap();
    let request = LaunchRequest::new(Mode::Guest, None).unwrap();
    let error = format!("{:#}", store.launch_guest(&app, &request).unwrap_err());
    assert!(error.contains("still uses the RAM disk"), "{error}");
    let sessions = guest_sessions(home.path());
    let session = fs::read_dir(&sessions)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .find(|path| {
            path.file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with("g-")
        })
        .unwrap();
    let volume = session.join("volume");
    assert!(volume.join("data/SingletonLock").symlink_metadata().is_ok());
    let pid: i32 = fs::read_to_string(&survivor).unwrap().parse().unwrap();
    fs::write(app.join("Contents/MacOS/Stainless"), b"#!/bin/sh\nexit 0\n").unwrap();
    store.launch_guest(&app, &request).unwrap();
    assert!(
        volume.join("data").exists(),
        "a live browser keeps its volume"
    );
    assert_eq!(guest_session_count(home.path()), 1);
    assert_eq!(unsafe { libc::kill(pid, libc::SIGTERM) }, 0);
    let deadline = Instant::now() + Duration::from_secs(5);
    while unsafe { libc::kill(pid, 0) } == 0 {
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(20));
    }
    store.launch_guest(&app, &request).unwrap();
    assert!(!session.exists());
    assert_eq!(guest_session_count(home.path()), 0);
}
