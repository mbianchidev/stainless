use fs2::FileExt;
use serde_json::{Value, json};
use stainless_profiles::{Store, files, native::HELPER_PATH};
use std::fs;
use std::io::{BufRead, BufReader, Read, Write};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdout, Command, Output, Stdio};

struct Fixture {
    home: tempfile::TempDir,
    app: PathBuf,
    helper: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let home = tempfile::tempdir().unwrap();
        let app = home.path().join("Applications/Stainless.app");
        let helper = app.join(HELPER_PATH);
        fs::create_dir_all(helper.parent().unwrap()).unwrap();
        fs::create_dir_all(app.join("Contents/MacOS")).unwrap();
        fs::copy(env!("CARGO_BIN_EXE_stainless-update"), &helper).unwrap();
        plist::to_file_xml(
            app.join("Contents/Info.plist"),
            &json!({
                "CFBundleIdentifier": "dev.stainless.browser",
                "CFBundleExecutable": "Stainless",
                "CrProductDirName": "Stainless",
                "CFBundleShortVersionString": "157.0.8103.2",
                "StainlessVersion": "0.1.0-preview.2",
                "StainlessBuild": 2,
                "StainlessChannel": "preview",
                "StainlessProfileSchema": 1,
                "StainlessManagedModesVersion": 1
            }),
        )
        .unwrap();
        plist::to_file_xml(
            app.join("Contents/Helpers/Stainless Updater.app/Contents/Info.plist"),
            &json!({
                "CFBundleIdentifier": "dev.stainless.browser.updater",
                "StainlessProfileChooserVersion": 1,
                "StainlessProfileRecoveryVersion": 1
            }),
        )
        .unwrap();
        let fixture = Self { home, app, helper };
        fixture.browser("#!/bin/sh\nexit 0\n");
        fixture
    }

    fn browser(&self, source: &str) {
        let executable = self.app.join("Contents/MacOS/Stainless");
        fs::write(&executable, source).unwrap();
        fs::set_permissions(executable, fs::Permissions::from_mode(0o700)).unwrap();
    }

    fn command(&self, operation: &str) -> Command {
        let mut command = Command::new(&self.helper);
        command
            .args([operation, "--app"])
            .arg(&self.app)
            .env_clear()
            .env("HOME", self.home.path())
            .env("PATH", self.home.path().join("no-developer-tools"))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        command
    }

    fn invoke(&self, operation: &str, request: &Value) -> Output {
        self.invoke_bytes(operation, &serde_json::to_vec(request).unwrap())
    }

    fn invoke_bytes(&self, operation: &str, bytes: &[u8]) -> Output {
        let mut child = self.command(operation).spawn().unwrap();
        child.stdin.take().unwrap().write_all(bytes).unwrap();
        child.wait_with_output().unwrap()
    }

    fn metadata(&self, request: Value) -> Value {
        succeeded(self.invoke(
            "profile-metadata",
            &json!({
                "protocol_version": 1,
                "request": request
            }),
        ))["metadata"]
            .clone()
    }

    fn create(&self, label: &str) -> String {
        self.metadata(json!({"operation":"create","label":label}))["profiles"][0]["id"]
            .as_str()
            .unwrap()
            .to_owned()
    }

    fn recovery(&self, request: Value) -> Output {
        self.invoke(
            "profile-recovery",
            &json!({"protocol_version":1,"request":request}),
        )
    }

    fn seeded_profile(&self, label: &str) -> String {
        let id = self.create(label);
        Store::for_home(self.home.path())
            .unwrap()
            .run(&id.clone().try_into().unwrap(), &self.app)
            .unwrap();
        let data = self.data(&id);
        fs::create_dir(data.join("Default")).unwrap();
        fs::write(data.join("Last Version"), b"157.0.8103.2").unwrap();
        fs::write(
            data.join("Default/Bookmarks"),
            b"synthetic saved browsing content",
        )
        .unwrap();
        id
    }

    fn launch_request(&self, id: &str, mode: &str) -> Value {
        json!({"protocol_version":1,"profile_id":id,"mode":mode})
    }

    fn start(&self, id: &str, mode: &str) -> (Child, BufReader<ChildStdout>) {
        let mut child = self.command("profile-launch").spawn().unwrap();
        child
            .stdin
            .take()
            .unwrap()
            .write_all(&serde_json::to_vec(&self.launch_request(id, mode)).unwrap())
            .unwrap();
        let mut output = BufReader::new(child.stdout.take().unwrap());
        let mut line = String::new();
        output.read_line(&mut line).unwrap();
        let reply: Value = serde_json::from_str(&line).unwrap();
        assert_eq!(reply["state"], "started", "{reply}");
        assert!(child.try_wait().unwrap().is_none());
        (child, output)
    }

    fn data(&self, id: &str) -> PathBuf {
        self.root().join("profiles").join(id).join("data")
    }

    fn root(&self) -> PathBuf {
        fs::canonicalize(self.home.path())
            .unwrap()
            .join("Library/Application Support/Stainless Profiles")
    }

    fn installation_lock(&self) -> fs::File {
        files::open_lock(
            &self
                .home
                .path()
                .join("Library/Application Support/Stainless Updater/install.lock"),
        )
        .unwrap()
    }
}

fn succeeded(output: Output) -> Value {
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let reply: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(reply["protocol_version"], 1);
    reply
}

fn failed(output: Output, diagnostic: &str) {
    assert!(!output.status.success());
    let replies: Vec<Value> = String::from_utf8(output.stdout)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    let error = replies.last().unwrap();
    assert_eq!(error["state"], "error");
    assert!(
        error["message"].as_str().unwrap().contains(diagnostic),
        "{error}"
    );
    assert!(!output.stderr.is_empty());
}

#[test]
fn bundled_metadata_lists_and_creates_without_preview_data_or_update_authorization() {
    let fixture = Fixture::new();
    let preview = fixture
        .home
        .path()
        .join("Library/Application Support/Stainless/Default");
    fs::create_dir_all(&preview).unwrap();
    fs::write(preview.join("Bookmarks"), b"synthetic untouched preview").unwrap();
    assert_eq!(
        fixture.metadata(json!({"operation":"list"}))["profiles"],
        json!([])
    );
    let id = fixture.create("Synthetic local");
    let metadata = fixture.metadata(json!({"operation":"list"}));
    assert_eq!(metadata["profiles"][0]["id"], id);
    assert_eq!(metadata["profiles"][0]["label"], "Synthetic local");
    assert_eq!(metadata["registry_version"], 1);
    assert_eq!(metadata["protocol_version"], 1);
    let registry = fs::read(fixture.root().join("registry.json")).unwrap();
    for rejected in [
        json!({"protocol_version":1,"request":{"operation":"launch","profile_id":id}}),
        json!({"protocol_version":1,"request":{"operation":"list","path":"/tmp/History"}}),
        json!({"protocol_version":1,"request":{"operation":"create","label":""}}),
    ] {
        failed(fixture.invoke("profile-metadata", &rejected), "");
    }
    failed(
        fixture.invoke_bytes("profile-metadata", &vec![b' '; 16 * 1024 + 1]),
        "too large",
    );
    assert_eq!(
        fs::read(fixture.root().join("registry.json")).unwrap(),
        registry
    );
    assert_eq!(
        fs::read(preview.join("Bookmarks")).unwrap(),
        b"synthetic untouched preview"
    );
    assert!(
        !fixture
            .home
            .path()
            .join("Library/Application Support/Stainless Updater/status.json")
            .exists()
    );
}

#[test]
fn bundled_recovery_backs_up_lists_only_metadata_and_restores_a_new_identity_without_overwrite() {
    let fixture = Fixture::new();
    let id = fixture.seeded_profile("Synthetic original");
    let store = Store::for_home(fixture.home.path()).unwrap();
    let data = fixture.data(&id);
    let cache = data.parent().unwrap().join("cache");
    fs::write(cache.join("synthetic-cache"), b"not backed up").unwrap();
    let backup =
        succeeded(fixture.recovery(json!({"operation":"backup","profile_id":id,"confirmed":true})));
    assert_eq!(backup["state"], "backed_up");
    let backup_id = backup["backup_id"].as_str().unwrap();
    let snapshot = fixture.root().join("backups").join(backup_id);
    assert!(snapshot.join("snapshot.json").is_file());
    assert_eq!(
        fs::read(snapshot.join("data/Default/Bookmarks")).unwrap(),
        b"synthetic saved browsing content"
    );
    assert!(!snapshot.join("cache").exists());
    let catalog = succeeded(fixture.recovery(json!({"operation":"list"})));
    assert_eq!(catalog["state"], "backups");
    let listed = &catalog["catalog"]["backups"][0];
    assert_eq!(listed["id"], backup_id);
    assert_eq!(listed["profile"]["id"], id);
    assert_eq!(listed["profile"]["label"], "Synthetic original");
    assert_eq!(listed["profile"]["last_browser"]["product"]["build"], 2);
    assert_eq!(listed["compatible"], true);
    assert!(listed["modified_unix_seconds"].as_u64().unwrap() > 0);
    assert!(catalog["catalog"]["next_after"].is_null());
    let metadata = serde_json::to_string(&catalog).unwrap();
    for forbidden in [
        "Bookmarks",
        "browsing content",
        "entries",
        "sha256",
        "cache",
    ] {
        assert!(!metadata.contains(forbidden), "{metadata}");
    }
    let registry = fs::read(fixture.root().join("registry.json")).unwrap();
    for operation in ["backup", "restore", "backups"] {
        failed(
            fixture.invoke(
                "profile-metadata",
                &json!({"protocol_version":1,"request":{"operation":operation}}),
            ),
            "metadata-only",
        );
    }
    assert_eq!(
        fs::read(fixture.root().join("registry.json")).unwrap(),
        registry
    );
    fs::write(
        data.join("Default/Bookmarks"),
        b"synthetic failed live state",
    )
    .unwrap();
    let failed_root = data.parent().unwrap().join("failed-synthetic");
    fs::create_dir(&failed_root).unwrap();
    fs::write(failed_root.join("retained"), b"synthetic failed root").unwrap();
    let unexpected_launch = fixture.home.path().join("unexpected-launch");
    fixture.browser(&format!(
        "#!/bin/sh\nprintf launched > '{}'\nexit 91\n",
        unexpected_launch.display()
    ));
    let restored = succeeded(fixture.recovery(json!({
        "operation":"restore","backup_id":backup_id,"label":"Synthetic recovered","confirmed":true
    })));
    assert_eq!(restored["state"], "restored");
    let restored = &restored["metadata"]["profiles"][0];
    let restored_id = restored["id"].as_str().unwrap();
    assert_ne!(restored_id, id);
    assert_eq!(restored["label"], "Synthetic recovered");
    assert_eq!(restored["last_browser"], listed["profile"]["last_browser"]);
    assert_eq!(
        fs::read(fixture.data(restored_id).join("Default/Bookmarks")).unwrap(),
        b"synthetic saved browsing content"
    );
    assert_eq!(
        fs::read(data.join("Default/Bookmarks")).unwrap(),
        b"synthetic failed live state"
    );
    assert_eq!(
        fs::read(failed_root.join("retained")).unwrap(),
        b"synthetic failed root"
    );
    assert!(snapshot.join("snapshot.json").is_file());
    assert_eq!(store.list().unwrap().len(), 2);
    assert!(!unexpected_launch.exists());
}

#[test]
fn native_recovery_requires_confirmation_selection_and_supported_narrow_authority() {
    let fixture = Fixture::new();
    let id = fixture.create("Synthetic");
    let registry = fs::read(fixture.root().join("registry.json")).unwrap();
    for request in [
        json!({"operation":"backup","profile_id":id,"confirmed":false}),
        json!({"operation":"backup","profile_id":id}),
        json!({"operation":"backup","confirmed":true}),
        json!({"operation":"backup","profile_id":"../preview","confirmed":true}),
        json!({"operation":"backup","profile_id":id,"confirmed":true,"path":"/tmp/History"}),
        json!({"operation":"backup","profile_id":id,"confirmed":true,"arguments":["--no-sandbox"]}),
        json!({"operation":"restore","backup_id":"b-1234567890123456","label":"Synthetic","confirmed":false}),
        json!({"operation":"restore","label":"Synthetic","confirmed":true}),
        json!({"operation":"restore","backup_id":"b-1234567890123456","label":"","confirmed":true}),
        json!({"operation":"restore","backup_id":"b-1234567890123456","label":"Synthetic","confirmed":true,"overwrite":id}),
        json!({"operation":"list","after":"../snapshot.json"}),
        json!({"operation":"list","inventory":true}),
        json!({"operation":"import","confirmed":true}),
    ] {
        failed(fixture.recovery(request), "");
    }
    failed(
        fixture.invoke(
            "profile-recovery",
            &json!({"protocol_version":2,"request":{"operation":"list"}}),
        ),
        "Unsupported native profile recovery protocol",
    );
    failed(
        fixture.invoke_bytes("profile-recovery", &vec![b' '; 16 * 1024 + 1]),
        "too large",
    );
    let helper_info = fixture
        .app
        .join("Contents/Helpers/Stainless Updater.app/Contents/Info.plist");
    let mut info = plist::Value::from_file(&helper_info).unwrap();
    info.as_dictionary_mut()
        .unwrap()
        .remove("StainlessProfileRecoveryVersion");
    plist::to_file_xml(helper_info, &info).unwrap();
    failed(
        fixture.recovery(json!({"operation":"list"})),
        "native profile recovery version 1",
    );
    assert_eq!(
        fixture.metadata(json!({"operation":"list"}))["profiles"][0]["id"],
        id
    );
    assert_eq!(
        fs::read(fixture.root().join("registry.json")).unwrap(),
        registry
    );
    assert!(
        fs::read_dir(fixture.root().join("backups"))
            .unwrap()
            .next()
            .is_none()
    );
}

#[test]
fn native_restore_rechecks_browser_floor_and_file_integrity_without_registering_partial_data() {
    let fixture = Fixture::new();
    let id = fixture.seeded_profile("Synthetic newer");
    let app_info = fixture.app.join("Contents/Info.plist");
    let original_app = fs::read(&app_info).unwrap();
    let mut newer = plist::Value::from_file(&app_info).unwrap();
    let dictionary = newer.as_dictionary_mut().unwrap();
    dictionary.insert("StainlessBuild".into(), 3.into());
    dictionary.insert("StainlessVersion".into(), "0.1.0-preview.3".into());
    dictionary.insert("CFBundleShortVersionString".into(), "157.0.8103.3".into());
    plist::to_file_xml(&app_info, &newer).unwrap();
    let store = Store::for_home(fixture.home.path()).unwrap();
    store
        .run(&id.clone().try_into().unwrap(), &fixture.app)
        .unwrap();
    fs::write(fixture.data(&id).join("Last Version"), b"157.0.8103.3").unwrap();
    let backup =
        succeeded(fixture.recovery(json!({"operation":"backup","profile_id":id,"confirmed":true})));
    let backup_id = backup["backup_id"].as_str().unwrap();
    let registry = fs::read(fixture.root().join("registry.json")).unwrap();
    fs::write(&app_info, original_app).unwrap();
    let catalog = succeeded(fixture.recovery(json!({"operation":"list"})));
    let listed = &catalog["catalog"]["backups"][0];
    assert_eq!(listed["compatible"], false);
    assert_eq!(listed["profile"]["last_browser"]["product"]["build"], 3);
    let request = json!({
        "operation":"restore","backup_id":backup_id,"label":"Must not register","confirmed":true
    });
    failed(fixture.recovery(request.clone()), "downgrade");
    assert_eq!(
        fs::read(fixture.root().join("registry.json")).unwrap(),
        registry
    );
    plist::to_file_xml(&app_info, &newer).unwrap();
    let snapshot = fixture.root().join("backups").join(backup_id);
    fs::write(
        snapshot.join("data/Default/Bookmarks"),
        b"synthetic corrupted copy",
    )
    .unwrap();
    failed(fixture.recovery(request.clone()), "corrupted or changed");
    let marker = snapshot.join("snapshot.json");
    let mut manifest: Value = serde_json::from_slice(&fs::read(&marker).unwrap()).unwrap();
    manifest["schema_version"] = 2.into();
    fs::write(&marker, serde_json::to_vec(&manifest).unwrap()).unwrap();
    let catalog = succeeded(fixture.recovery(json!({"operation":"list"})));
    assert_eq!(catalog["catalog"]["backups"][0]["compatible"], false);
    failed(fixture.recovery(request), "Unsupported backup format");
    assert_eq!(
        fs::read(fixture.root().join("registry.json")).unwrap(),
        registry
    );
    assert_eq!(
        fs::read_dir(fixture.root().join("profiles"))
            .unwrap()
            .count(),
        1
    );
    assert_eq!(
        fs::read(fixture.data(&id).join("Default/Bookmarks")).unwrap(),
        b"synthetic saved browsing content"
    );
    assert!(marker.exists());
}

#[test]
fn native_cold_operations_wait_for_owned_browsers_and_refuse_pending_recovery() {
    let fixture = Fixture::new();
    let id = fixture.create("Synthetic");
    let backup =
        succeeded(fixture.recovery(json!({"operation":"backup","profile_id":id,"confirmed":true})));
    let backup_id = backup["backup_id"].as_str().unwrap();
    let backup_request = json!({"operation":"backup","profile_id":id,"confirmed":true});
    let restore_request = json!({
        "operation":"restore","backup_id":backup_id,"label":"Synthetic clone","confirmed":true
    });
    fixture.browser(
        "#!/bin/sh\nset -eu\ndata=\"${1#--user-data-dir=}\"\nprintf '157.0.8103.2' > \"$data/Last Version\"\ni=0\nwhile [ ! -f \"$data/stop\" ] && [ \"$i\" -lt 600 ]; do i=$((i + 1)); /bin/sleep 0.02; done\n",
    );
    let (mut owner, _owner_output) = fixture.start(&id, "standard");
    for request in [&backup_request, &restore_request] {
        failed(
            fixture.recovery(request.clone()),
            "Quit every Stainless browser",
        );
    }
    assert_eq!(
        succeeded(fixture.recovery(json!({"operation":"list"})))["catalog"]["backups"][0]["id"],
        backup_id
    );
    fs::write(fixture.data(&id).join("stop"), b"").unwrap();
    assert!(owner.wait().unwrap().success());
    let root_lock =
        files::open_lock(&fixture.data(&id).parent().unwrap().join("owner.lock")).unwrap();
    root_lock.try_lock_exclusive().unwrap();
    failed(fixture.recovery(backup_request.clone()), "already owned");
    drop(root_lock);
    let transaction = fixture
        .home
        .path()
        .join("Library/Application Support/Stainless Updater/transaction.json");
    let registry = fs::read(fixture.root().join("registry.json")).unwrap();
    for phase in ["prepared", "awaiting_health", "rolling_back", "unknown"] {
        fs::write(
            &transaction,
            serde_json::to_vec(&json!({"phase":phase})).unwrap(),
        )
        .unwrap();
        for request in [&backup_request, &restore_request] {
            failed(fixture.recovery(request.clone()), "pending preview update");
        }
        assert_eq!(
            succeeded(fixture.recovery(json!({"operation":"list"})))["catalog"]["backups"][0]["id"],
            backup_id
        );
        assert_eq!(
            fs::read(fixture.root().join("registry.json")).unwrap(),
            registry
        );
    }
    fs::remove_file(transaction).unwrap();
    assert_eq!(
        succeeded(fixture.recovery(backup_request))["state"],
        "backed_up"
    );
    assert_eq!(
        succeeded(fixture.recovery(restore_request))["state"],
        "restored"
    );
}

#[test]
fn native_catalog_pages_and_exposes_retained_incomplete_backups_without_reading_browsing_data() {
    use std::os::unix::fs::symlink;

    let fixture = Fixture::new();
    let id = fixture.seeded_profile("Synthetic interrupted");
    let outside = fixture.home.path().join("synthetic-external");
    fs::write(&outside, b"synthetic unchanged external data").unwrap();
    symlink(&outside, fixture.data(&id).join("Default/unexpected-link")).unwrap();
    failed(
        fixture.recovery(json!({"operation":"backup","profile_id":id,"confirmed":true})),
        "Backup state retained",
    );
    let retained = fs::read_dir(fixture.root().join("backups"))
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    assert!(!retained.join("snapshot.json").exists());
    for index in 0..65 {
        fs::create_dir(
            fixture
                .root()
                .join("backups")
                .join(format!("b-{index:016}")),
        )
        .unwrap();
    }
    let mut after = None;
    let mut listed = std::collections::HashSet::new();
    loop {
        let reply = succeeded(fixture.recovery(json!({"operation":"list","after":after})));
        let catalog = &reply["catalog"];
        let backups = catalog["backups"].as_array().unwrap();
        assert!(backups.len() <= 64);
        for backup in backups {
            assert_eq!(backup["compatible"], false);
            assert!(backup["profile"].is_null());
            assert!(listed.insert(backup["id"].as_str().unwrap().to_owned()));
            assert!(backup["message"].as_str().unwrap().contains("incomplete"));
        }
        after = catalog["next_after"].as_str().map(str::to_owned);
        if after.is_none() {
            break;
        }
    }
    assert_eq!(listed.len(), 66);
    failed(
        fixture.recovery(json!({
            "operation":"restore","backup_id":"b-0000000000000000","label":"Must not register","confirmed":true
        })),
        "Cannot read profile file",
    );
    assert_eq!(
        fs::read(&outside).unwrap(),
        b"synthetic unchanged external data"
    );
    assert_eq!(
        Store::for_home(fixture.home.path())
            .unwrap()
            .list()
            .unwrap()
            .len(),
        1
    );
    assert!(retained.exists());
}

#[test]
fn disconnected_recovery_acknowledgements_report_committed_state_and_retain_it_for_refresh() {
    let fixture = Fixture::new();
    let id = fixture.create("Synthetic");
    let disconnect = |request: Value| {
        let mut child = fixture.command("profile-recovery").spawn().unwrap();
        drop(child.stdout.take());
        child
            .stdin
            .take()
            .unwrap()
            .write_all(
                &serde_json::to_vec(&json!({"protocol_version":1,"request":request})).unwrap(),
            )
            .unwrap();
        let output = child.wait_with_output().unwrap();
        assert!(!output.status.success());
        assert!(String::from_utf8_lossy(&output.stderr).contains("acknowledgement failed"));
    };
    disconnect(json!({"operation":"backup","profile_id":id,"confirmed":true}));
    let catalog = succeeded(fixture.recovery(json!({"operation":"list"})));
    let backup_id = catalog["catalog"]["backups"][0]["id"].as_str().unwrap();
    assert_eq!(catalog["catalog"]["backups"][0]["compatible"], true);
    disconnect(json!({
        "operation":"restore","backup_id":backup_id,"label":"Synthetic retained clone","confirmed":true
    }));
    let profiles = fixture.metadata(json!({"operation":"list"}));
    assert_eq!(profiles["profiles"].as_array().unwrap().len(), 2);
    assert_eq!(profiles["profiles"][1]["label"], "Synthetic retained clone");
    assert!(
        fixture
            .data(profiles["profiles"][1]["id"].as_str().unwrap())
            .is_dir()
    );
    assert!(
        fixture
            .root()
            .join("backups")
            .join(backup_id)
            .join("snapshot.json")
            .exists()
    );
}

#[test]
fn native_launch_rejects_nonbundled_helpers_unsupported_apps_paths_and_extra_authority() {
    let fixture = Fixture::new();
    let request = json!({"protocol_version":1,"request":{"operation":"list"}});
    let mut command = fixture.command("profile-metadata");
    command.arg("--no-sandbox");
    failed(command.output().unwrap(), "only --app");
    let external = fixture.home.path().join("external-helper");
    fs::copy(&fixture.helper, &external).unwrap();
    let mut child = Command::new(external)
        .args(["profile-metadata", "--app"])
        .arg(&fixture.app)
        .env("HOME", fixture.home.path())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(&serde_json::to_vec(&request).unwrap())
        .unwrap();
    failed(child.wait_with_output().unwrap(), "bundled helper");
    let info_path = fixture.app.join("Contents/Info.plist");
    let mut info = plist::Value::from_file(&info_path).unwrap();
    info.as_dictionary_mut()
        .unwrap()
        .remove("StainlessManagedModesVersion");
    plist::to_file_xml(&info_path, &info).unwrap();
    failed(
        fixture.invoke("profile-metadata", &request),
        "managed Incognito enforcement",
    );
    for request in [
        json!({"protocol_version":2,"profile_id":"p-1234567890123456","mode":"standard"}),
        json!({"protocol_version":1,"profile_id":"../preview","mode":"standard"}),
        json!({"protocol_version":1,"profile_id":"p-1234567890123456","mode":"guest"}),
        json!({"protocol_version":1,"profile_id":"p-1234567890123456","mode":"standard","url":"https://synthetic.invalid/"}),
        json!({"protocol_version":1,"profile_id":"p-1234567890123456","mode":"standard","arguments":["--no-sandbox"]}),
        json!({"protocol_version":1,"profile_id":"p-1234567890123456","mode":"standard","app":"/tmp/Stainless.app"}),
    ] {
        failed(fixture.invoke("profile-launch", &request), "");
    }
    assert!(!fixture.root().exists());
}

#[test]
fn native_supervisors_route_standard_and_fresh_incognito_and_hold_installation_ownership() {
    let fixture = Fixture::new();
    let first = fixture.create("Synthetic first");
    let second = fixture.create("Synthetic second");
    let records = fixture.home.path().join("records");
    fs::create_dir(&records).unwrap();
    fixture.browser(&format!(
        "#!/bin/sh\nset -eu\ndata=\"${{1#--user-data-dir=}}\"\nmode=\"${{3#--stainless-managed-mode=}}\"\nif [ \"$mode\" = incognito ]; then test ! -f \"$data/Cookies\"; test ! -f \"$data/Login Data\"; fi\nprintf '%s\\n' \"$@\" > '{}/'\"$$\"\nprintf '%s' \"$$\" > \"$data/pid\"\ni=0\nwhile [ ! -f \"$data/stop\" ] && [ \"$i\" -lt 600 ]; do i=$((i + 1)); /bin/sleep 0.02; done\n",
        records.display()
    ));
    let (mut owner, mut owner_output) = fixture.start(&first, "standard");
    let (mut other, mut other_output) = fixture.start(&second, "standard");
    fs::write(fixture.data(&first).join("Cookies"), b"synthetic cookie").unwrap();
    fs::write(
        fixture.data(&first).join("Login Data"),
        b"synthetic password",
    )
    .unwrap();
    let before = fs::read(fixture.root().join("registry.json")).unwrap();
    let (mut private, mut private_output) = fixture.start(&first, "incognito");
    failed(
        fixture.invoke(
            "profile-launch",
            &fixture.launch_request(&first, "standard"),
        ),
        "already owned",
    );
    let installation = fixture.installation_lock();
    assert!(installation.try_lock_exclusive().is_err());
    let sessions = fixture.data(&first).parent().unwrap().join("incognito");
    let private_directory = fs::read_dir(&sessions)
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    let private_data = private_directory.join("data");
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while !private_data.join("pid").exists() {
        assert!(std::time::Instant::now() < deadline);
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    assert_eq!(
        fs::read(fixture.root().join("registry.json")).unwrap(),
        before
    );
    assert!(!private_data.join("Cookies").exists());
    fs::write(private_data.join("stop"), b"").unwrap();
    assert!(private.wait().unwrap().success());
    assert!(!private_directory.exists());
    assert!(installation.try_lock_exclusive().is_err());
    for id in [&first, &second] {
        fs::write(fixture.data(id).join("stop"), b"").unwrap();
    }
    assert!(owner.wait().unwrap().success());
    assert!(other.wait().unwrap().success());
    installation.try_lock_exclusive().unwrap();
    for output in [&mut owner_output, &mut other_output, &mut private_output] {
        let mut line = String::new();
        output.read_to_string(&mut line).unwrap();
        assert_eq!(
            serde_json::from_str::<Value>(&line).unwrap()["state"],
            "exited"
        );
    }
    let mut modes = Vec::new();
    for record in fs::read_dir(records).unwrap() {
        let arguments = fs::read_to_string(record.unwrap().path()).unwrap();
        let arguments: Vec<_> = arguments.lines().collect();
        modes.push(arguments[2].to_owned());
        let data = Path::new(arguments[0].strip_prefix("--user-data-dir=").unwrap());
        let cache = Path::new(arguments[1].strip_prefix("--disk-cache-dir=").unwrap());
        assert_eq!(data.parent(), cache.parent());
        if arguments[2].ends_with("incognito") {
            assert_ne!(data, fixture.data(&first));
            assert_eq!(
                &arguments[3..],
                ["--incognito", "--disable-extensions", "--no-first-run"]
            );
        } else {
            assert!(data == fixture.data(&first) || data == fixture.data(&second));
            assert_eq!(arguments.len(), 3);
        }
    }
    modes.sort();
    assert_eq!(
        modes,
        [
            "--stainless-managed-mode=incognito",
            "--stainless-managed-mode=standard",
            "--stainless-managed-mode=standard"
        ]
    );
}

#[test]
fn pending_recovery_preserves_listing_but_blocks_creation_launch_and_first_registry_initialization()
{
    let fixture = Fixture::new();
    let id = fixture.create("Synthetic");
    let transaction = fixture
        .home
        .path()
        .join("Library/Application Support/Stainless Updater/transaction.json");
    let before = fs::read(fixture.root().join("registry.json")).unwrap();
    for phase in ["prepared", "installed", "rolling_back", "unknown"] {
        fs::write(
            &transaction,
            serde_json::to_vec(&json!({"phase":phase})).unwrap(),
        )
        .unwrap();
        assert_eq!(
            fixture.metadata(json!({"operation":"list"}))["profiles"][0]["id"],
            id
        );
        failed(
            fixture.invoke(
                "profile-metadata",
                &json!({
                    "protocol_version":1,"request":{"operation":"create","label":"Blocked"}
                }),
            ),
            "pending preview update",
        );
        for mode in ["standard", "incognito"] {
            failed(
                fixture.invoke("profile-launch", &fixture.launch_request(&id, mode)),
                "pending preview update",
            );
        }
        assert_eq!(
            fs::read(fixture.root().join("registry.json")).unwrap(),
            before
        );
    }
    let first_use = Fixture::new();
    let state = first_use
        .home
        .path()
        .join("Library/Application Support/Stainless Updater");
    fs::create_dir_all(&state).unwrap();
    fs::write(
        state.join("transaction.json"),
        br#"{"phase":"rolling_back"}"#,
    )
    .unwrap();
    failed(
        first_use.invoke(
            "profile-metadata",
            &json!({
                "protocol_version":1,"request":{"operation":"list"}
            }),
        ),
        "pending preview update",
    );
    assert!(!first_use.root().exists());
    fs::remove_file(transaction).unwrap();
    let installation = fixture.installation_lock();
    installation.try_lock_exclusive().unwrap();
    failed(
        fixture.invoke(
            "profile-metadata",
            &json!({
                "protocol_version":1,"request":{"operation":"list"}
            }),
        ),
        "finish any update",
    );
    assert_eq!(
        fs::read(fixture.root().join("registry.json")).unwrap(),
        before
    );
}

#[test]
fn launch_errors_are_structured_and_private_failure_cleanup_does_not_change_persistent_identity() {
    let fixture = Fixture::new();
    let id = fixture.create("Synthetic");
    fixture.browser("#!/bin/sh\nexit 9\n");
    let before = Store::for_home(fixture.home.path())
        .unwrap()
        .list()
        .unwrap();
    failed(
        fixture.invoke("profile-launch", &fixture.launch_request(&id, "incognito")),
        "exited unsuccessfully",
    );
    assert_eq!(
        Store::for_home(fixture.home.path())
            .unwrap()
            .list()
            .unwrap(),
        before
    );
    assert!(
        fs::read_dir(fixture.data(&id).parent().unwrap().join("incognito"))
            .unwrap()
            .next()
            .is_none()
    );
    fixture.browser(
        "#!/bin/sh\nset -eu\ndata=\"${1#--user-data-dir=}\"\n/bin/mkdir \"$data/retained\"\nprintf 'synthetic retained state' > \"$data/retained/file\"\n/bin/chmod 500 \"$data/retained\"\n",
    );
    failed(
        fixture.invoke("profile-launch", &fixture.launch_request(&id, "incognito")),
        "Cannot remove the private session",
    );
    let sessions = fixture.data(&id).parent().unwrap().join("incognito");
    let retained = fs::read_dir(&sessions)
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    assert_eq!(
        fs::read(retained.join("data/retained/file")).unwrap(),
        b"synthetic retained state"
    );
    fs::set_permissions(
        retained.join("data/retained"),
        fs::Permissions::from_mode(0o700),
    )
    .unwrap();
    fs::remove_dir_all(&retained).unwrap();
    fs::remove_file(fixture.app.join("Contents/MacOS/Stainless")).unwrap();
    failed(
        fixture.invoke("profile-launch", &fixture.launch_request(&id, "standard")),
        "Cannot start the owned browser",
    );
    assert!(
        Store::for_home(fixture.home.path())
            .unwrap()
            .list()
            .unwrap()[0]
            .last_browser
            .is_some()
    );
}

#[test]
fn a_disconnected_native_window_does_not_release_the_foreground_browser_owner_early() {
    let fixture = Fixture::new();
    let id = fixture.create("Synthetic");
    fixture.browser(
        "#!/bin/sh\nset -eu\ndata=\"${1#--user-data-dir=}\"\nprintf 'started' > \"$data/fixture-started\"\ni=0\nwhile [ ! -f \"$data/stop\" ] && [ \"$i\" -lt 600 ]; do i=$((i + 1)); /bin/sleep 0.02; done\n",
    );
    let mut child = fixture.command("profile-launch").spawn().unwrap();
    drop(child.stdout.take());
    child
        .stdin
        .take()
        .unwrap()
        .write_all(&serde_json::to_vec(&fixture.launch_request(&id, "standard")).unwrap())
        .unwrap();
    let data = fixture.data(&id);
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while !data.join("fixture-started").exists() {
        assert!(std::time::Instant::now() < deadline);
        assert!(child.try_wait().unwrap().is_none());
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    let installation = fixture.installation_lock();
    assert!(installation.try_lock_exclusive().is_err());
    failed(
        fixture.invoke("profile-launch", &fixture.launch_request(&id, "standard")),
        "already owned",
    );
    fs::write(data.join("stop"), b"").unwrap();
    assert!(!child.wait().unwrap().success());
    installation.try_lock_exclusive().unwrap();
}
