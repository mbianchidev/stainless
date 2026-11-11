use super::*;
use crate::tests::{browser, data, synthetic_app};
use std::fs;
use std::path::{Path, PathBuf};

fn preview(home: &Path) -> PathBuf {
    let preview = home.join("Library/Application Support/Stainless");
    fs::create_dir_all(preview.join("Default")).unwrap();
    fs::write(preview.join("Last Version"), b"157.0.8103.2").unwrap();
    fs::write(
        preview.join("Default/Bookmarks"),
        b"synthetic preview bookmarks",
    )
    .unwrap();
    preview
}

fn snapshot(home: &Path, id: &BackupId) -> PathBuf {
    home.join("Library/Application Support/Stainless Profiles/backups")
        .join(id.to_string())
        .join("data")
}

#[test]
fn imports_via_a_verified_backup_and_restores_into_a_new_profile_without_overwriting_data() {
    let home = tempfile::tempdir().unwrap();
    let store = Store::for_home(home.path()).unwrap();
    let preview = preview(home.path());
    let app = synthetic_app(home.path(), &browser(2));
    let imported = store.import_preview("Synthetic import", &app).unwrap();
    let original = fs::read(preview.join("Default/Bookmarks")).unwrap();
    let managed = data(home.path(), &imported.profile.id);
    assert_eq!(
        fs::read(managed.join("Default/Bookmarks")).unwrap(),
        original
    );
    assert_eq!(
        fs::read(snapshot(home.path(), &imported.backup).join("Default/Bookmarks")).unwrap(),
        original
    );
    assert_eq!(imported.profile.last_browser, Some(browser(2)));
    fs::write(managed.join("Default/Bookmarks"), b"synthetic saved state").unwrap();
    let backup = store.backup(&imported.profile.id).unwrap();
    fs::write(managed.join("Default/Bookmarks"), b"synthetic failed state").unwrap();
    store
        .run(
            &imported.profile.id,
            &synthetic_app(home.path(), &browser(3)),
        )
        .unwrap();
    assert!(store.run(&imported.profile.id, &app).is_err());
    let restored = store.restore(&backup, "Synthetic restored").unwrap();
    assert_ne!(imported.profile.id, restored.id);
    assert_eq!(
        fs::read(data(home.path(), &restored.id).join("Default/Bookmarks")).unwrap(),
        b"synthetic saved state"
    );
    assert_eq!(
        fs::read(managed.join("Default/Bookmarks")).unwrap(),
        b"synthetic failed state"
    );
    assert_eq!(
        fs::read(preview.join("Default/Bookmarks")).unwrap(),
        original
    );
    assert_eq!(restored.last_browser, Some(browser(2)));
    store.run(&restored.id, &app).unwrap();
    fs::write(
        snapshot(home.path(), &backup).join("Default/Bookmarks"),
        b"synthetic corrupted backup",
    )
    .unwrap();
    assert!(store.restore(&backup, "Must not register").is_err());
    assert_eq!(store.list().unwrap().len(), 2);
}

#[test]
fn cold_operations_share_the_updater_lock_and_refuse_pending_or_unknown_transactions() {
    use fs2::FileExt;

    let home = tempfile::tempdir().unwrap();
    let store = Store::for_home(home.path()).unwrap();
    let preview = preview(home.path());
    let app = synthetic_app(home.path(), &browser(2));
    let imported = store.import_preview("Synthetic", &app).unwrap();
    let state = home
        .path()
        .join("Library/Application Support/Stainless Updater");
    let shared = crate::files::open_lock(&state.join("install.lock")).unwrap();
    FileExt::lock_shared(&shared).unwrap();
    assert!(store.import_preview("Blocked import", &app).is_err());
    assert!(store.backup(&imported.profile.id).is_err());
    assert!(store.restore(&imported.backup, "Blocked restore").is_err());
    drop(shared);
    for phase in ["prepared", "installed", "unknown"] {
        fs::write(
            state.join("transaction.json"),
            serde_json::to_vec(&serde_json::json!({"phase": phase})).unwrap(),
        )
        .unwrap();
        assert!(store.import_preview("Blocked import", &app).is_err());
        assert!(store.backup(&imported.profile.id).is_err());
        assert!(store.restore(&imported.backup, "Blocked restore").is_err());
        assert!(store.run(&imported.profile.id, &app).is_err());
    }
    assert_eq!(store.list().unwrap().len(), 1);
    assert_eq!(
        fs::read(preview.join("Default/Bookmarks")).unwrap(),
        b"synthetic preview bookmarks"
    );
    fs::write(state.join("transaction.json"), br#"{"phase":"healthy"}"#).unwrap();
    store.backup(&imported.profile.id).unwrap();
}

#[test]
fn failed_imports_and_uncommitted_backups_never_replace_or_register_partial_data() {
    use std::os::unix::fs::symlink;

    let home = tempfile::tempdir().unwrap();
    let store = Store::for_home(home.path()).unwrap();
    let preview = preview(home.path());
    let app = synthetic_app(home.path(), &browser(2));
    let outside = home.path().join("synthetic-outside");
    fs::write(&outside, b"synthetic unrelated file").unwrap();
    symlink(&outside, preview.join("Default/unexpected-link")).unwrap();
    assert!(store.import_preview("Must fail safely", &app).is_err());
    assert!(store.list().unwrap().is_empty());
    let backups = home
        .path()
        .join("Library/Application Support/Stainless Profiles/backups");
    let incomplete = fs::read_dir(&backups)
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    assert!(!incomplete.join("snapshot.json").exists());
    assert_eq!(fs::read(&outside).unwrap(), b"synthetic unrelated file");
    assert_eq!(
        fs::read(preview.join("Default/Bookmarks")).unwrap(),
        b"synthetic preview bookmarks"
    );
    fs::remove_file(preview.join("Default/unexpected-link")).unwrap();
    symlink(&outside, preview.join("SingletonCookie")).unwrap();
    symlink("157.0.8103.2", preview.join("RunningChromeVersion")).unwrap();
    let imported = store.import_preview("Synthetic complete", &app).unwrap();
    assert!(
        !data(home.path(), &imported.profile.id)
            .join("SingletonCookie")
            .exists()
    );
    assert!(incomplete.is_dir());
    let committed = snapshot(home.path(), &imported.backup)
        .parent()
        .unwrap()
        .to_owned();
    let marker = committed.join("snapshot.json");
    let mut metadata: serde_json::Value =
        serde_json::from_slice(&fs::read(&marker).unwrap()).unwrap();
    metadata["schema_version"] = 2.into();
    fs::write(&marker, serde_json::to_vec(&metadata).unwrap()).unwrap();
    assert!(store.restore(&imported.backup, "Future backup").is_err());
    fs::remove_file(&marker).unwrap();
    assert!(
        store
            .restore(&imported.backup, "Uncommitted backup")
            .is_err()
    );
    assert_eq!(store.list().unwrap().len(), 1);
    assert!(committed.join("data/Default/Bookmarks").is_file());
}

#[test]
fn preview_pairing_and_orphaned_chromium_ownership_fail_closed() {
    use std::os::unix::fs::symlink;

    let home = tempfile::tempdir().unwrap();
    let store = Store::for_home(home.path()).unwrap();
    let preview = preview(home.path());
    let app = synthetic_app(home.path(), &browser(2));
    assert!(
        store
            .import_preview("Wrong app", &synthetic_app(home.path(), &browser(1)))
            .is_err()
    );
    let mut changed_schema = browser(2);
    changed_schema.product.profile_schema = 2;
    assert!(
        store
            .import_preview(
                "Unknown migration",
                &synthetic_app(home.path(), &changed_schema)
            )
            .is_err()
    );
    symlink("synthetic-host-424242", preview.join("SingletonLock")).unwrap();
    assert!(store.import_preview("Owned preview", &app).is_err());
    fs::remove_file(preview.join("SingletonLock")).unwrap();
    let imported = store.import_preview("Synthetic", &app).unwrap();
    let managed = data(home.path(), &imported.profile.id);
    symlink("synthetic-host-424242", managed.join("SingletonLock")).unwrap();
    assert!(store.backup(&imported.profile.id).is_err());
    assert!(store.run(&imported.profile.id, &app).is_err());
    assert_eq!(store.list().unwrap().len(), 1);
    assert_eq!(
        fs::read(managed.join("Default/Bookmarks")).unwrap(),
        b"synthetic preview bookmarks"
    );
}
