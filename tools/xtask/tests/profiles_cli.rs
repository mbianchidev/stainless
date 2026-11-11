use serde_json::Value;
use stainless_profiles::protocol::MAX_REQUEST_BYTES;
use std::fs;
use std::io::Write;
use std::path::Path;
use std::process::{Command, Output, Stdio};

fn invoke(home: &Path, args: &[&str], input: Option<&[u8]>) -> Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_xtask"))
        .arg("profiles")
        .args(args)
        .env("HOME", home)
        .stdin(if input.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    if let Some(input) = input {
        child.stdin.take().unwrap().write_all(input).unwrap();
    }
    child.wait_with_output().unwrap()
}

fn success(output: Output) -> Value {
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap()
}

#[test]
fn profile_cli_persists_metadata_and_recovers_a_copy_without_touching_the_preview() {
    let home = tempfile::tempdir().unwrap();
    let preview = home
        .path()
        .join("Library/Application Support/Stainless/Default");
    fs::create_dir_all(&preview).unwrap();
    fs::write(preview.join("Bookmarks"), b"synthetic untouched preview").unwrap();
    assert_eq!(
        success(invoke(home.path(), &["list"], None)),
        serde_json::json!([])
    );
    let created = success(invoke(home.path(), &["create", "Synthetic work"], None));
    let profile = created["id"].as_str().unwrap();
    let backup = success(invoke(home.path(), &["backup", profile], None));
    let backup = backup.as_str().unwrap();
    let restored = success(invoke(
        home.path(),
        &["restore", backup, "Synthetic restored copy"],
        None,
    ));
    assert_ne!(restored["id"], created["id"]);
    assert_eq!(restored["profile_schema"], 1);
    let mut request = br#"{"protocol_version":1,"request":{"operation":"list"}}"#.to_vec();
    request.resize(MAX_REQUEST_BYTES, b' ');
    let response = success(invoke(home.path(), &["metadata"], Some(&request)));
    assert_eq!(response["protocol_version"], 1);
    assert_eq!(response["registry_version"], 1);
    assert_eq!(response["profiles"].as_array().unwrap().len(), 2);
    assert!(
        !String::from_utf8(serde_json::to_vec(&response).unwrap())
            .unwrap()
            .contains("untouched preview")
    );
    let registry = home
        .path()
        .join("Library/Application Support/Stainless Profiles/registry.json");
    let before = fs::read(&registry).unwrap();
    request.push(b' ');
    for rejected in [
        request.as_slice(),
        br#"{"protocol_version":1,"request":{"operation":"history","path":"/tmp/History"}}"#,
    ] {
        let output = invoke(home.path(), &["metadata"], Some(rejected));
        assert!(!output.status.success());
        assert!(!output.stderr.is_empty());
        assert!(output.stdout.is_empty());
        assert_eq!(fs::read(&registry).unwrap(), before);
    }

    assert_eq!(
        fs::read(preview.join("Bookmarks")).unwrap(),
        b"synthetic untouched preview"
    );
}

#[test]
fn mode_cli_reports_availability_and_rejects_unsafe_launches_before_creating_state() {
    let temporary = tempfile::tempdir().unwrap();
    let home = temporary.path().join("not-created");
    let modes = success(invoke(&home, &["modes"], None));
    assert_eq!(modes.as_array().unwrap().len(), 8);
    for mode in modes.as_array().unwrap() {
        let name = mode["mode"].as_str().unwrap();
        if matches!(name, "standard" | "incognito") {
            assert!(mode["unavailable_reason"].is_null());
        } else {
            let result = invoke(
                &home,
                &[
                    "run",
                    "/synthetic/Stainless.app",
                    "p-1234567890123456",
                    name,
                ],
                None,
            );
            assert!(!result.status.success());
            assert!(String::from_utf8_lossy(&result.stderr).contains("unavailable"));
        }
    }
    let rejected = invoke(
        &home,
        &[
            "open",
            "/synthetic/Stainless.app",
            "p-1234567890123456",
            "incognito",
            "https://user:password@synthetic.invalid/",
        ],
        None,
    );
    assert!(!rejected.status.success());
    assert!(String::from_utf8_lossy(&rejected.stderr).contains("credentials"));
    assert!(!home.exists());
}
