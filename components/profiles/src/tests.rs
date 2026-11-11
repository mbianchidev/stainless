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
