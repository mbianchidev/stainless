use anyhow::{Context, Result, bail, ensure};
use stainless_release::{BUNDLE_ID, Product};
use std::fs::{self, File};
use std::io::Read;
use std::os::unix::fs::{PermissionsExt, symlink};
use std::path::{Component, Path};
use std::process::Command;

pub fn product(app: &Path) -> Result<Product> {
    let info = plist::Value::from_file(app.join("Contents/Info.plist"))
        .context("Cannot read the Stainless app manifest")?;
    let info = info
        .as_dictionary()
        .context("App manifest is not a dictionary")?;
    let text = |key| -> Result<&str> {
        info.get(key)
            .and_then(plist::Value::as_string)
            .with_context(|| format!("Missing app manifest field {key}"))
    };
    ensure!(
        text("CFBundleIdentifier")? == BUNDLE_ID,
        "Wrong application identity"
    );
    ensure!(
        text("CFBundleExecutable")? == "Stainless",
        "Wrong browser executable"
    );
    ensure!(
        text("CrProductDirName")? == "Stainless",
        "Wrong browser profile identity"
    );
    let product: Product = serde_json::from_value(serde_json::json!({
        "version": text("StainlessVersion")?,
        "build": info.get("StainlessBuild").and_then(plist::Value::as_unsigned_integer)
            .context("Missing Stainless build number")?,
        "channel": text("StainlessChannel")?,
        "profile_schema": info.get("StainlessProfileSchema").and_then(plist::Value::as_unsigned_integer)
            .context("Missing profile schema")?,
    }))?;
    product.validate()?;
    Ok(product)
}

pub fn validate(app: &Path, expected: &Product) -> Result<()> {
    ensure!(
        &product(app)? == expected,
        "Payload app does not match the update feed"
    );
    let main = app.join("Contents/MacOS/Stainless");
    let mut header = [0; 8];
    File::open(&main)?.read_exact(&mut header)?;
    ensure!(
        header[..4] == [0xcf, 0xfa, 0xed, 0xfe]
            && u32::from_le_bytes(header[4..8].try_into()?) == 0x0100000c,
        "Browser executable is not an arm64 Mach-O binary"
    );
    ensure!(
        main.metadata()?.permissions().mode() & 0o111 != 0,
        "Browser is not executable"
    );
    let framework = app.join("Contents/Frameworks/Stainless Framework.framework");
    ensure!(framework.is_dir(), "Browser framework is missing");
    ensure!(
        framework
            .join("Versions/Current/Helpers/Stainless Helper (Renderer).app")
            .is_dir(),
        "Sandboxed renderer helper is missing"
    );
    ensure!(
        app.join("Contents/Helpers/Stainless Updater.app/Contents/Resources/stainless-update")
            .is_file(),
        "Updater is missing from the payload"
    );
    check_links(app, &fs::canonicalize(app)?)?;
    let result = Command::new("/usr/bin/codesign")
        .args(["--verify", "--deep", "--strict"])
        .arg(app)
        .output()
        .context("Cannot verify local executable signatures")?;
    ensure!(
        result.status.success(),
        "App integrity/ad-hoc signature verification failed"
    );
    Ok(())
}

fn check_links(path: &Path, root: &Path) -> Result<()> {
    for entry in fs::read_dir(path)? {
        let entry = entry?;
        let kind = entry.file_type()?;
        if kind.is_symlink() {
            ensure!(
                fs::canonicalize(entry.path())?.starts_with(root),
                "Bundle symlink escapes the application"
            );
        } else if kind.is_dir() {
            check_links(&entry.path(), root)?;
        }
    }
    Ok(())
}

pub fn copy_tree(source: &Path, destination: &Path) -> Result<()> {
    ensure!(
        !destination.exists(),
        "Backup/staging destination already exists"
    );
    let result = Command::new("/usr/bin/ditto")
        .arg(source)
        .arg(destination)
        .output()
        .context("Cannot start the macOS copy tool")?;
    ensure!(
        result.status.success(),
        "App/profile copy failed; check disk space and permissions"
    );
    Ok(())
}

pub fn unpack(archive: &Path, destination: &Path) -> Result<()> {
    ensure!(
        fs::read_dir(destination)?.next().is_none(),
        "Extraction directory must be empty"
    );
    let mut zip = zip::ZipArchive::new(File::open(archive)?).context("Invalid update ZIP")?;
    ensure!(zip.len() <= 50_000, "Update ZIP has too many entries");
    let mut total = 0u64;
    let mut links = Vec::new();
    for index in 0..zip.len() {
        let mut entry = zip.by_index(index)?;
        let name = entry.name()?.into_owned();
        validate_entry(&name)?;
        total = total
            .checked_add(entry.size())
            .context("ZIP size overflow")?;
        ensure!(
            total <= 8 * 1024 * 1024 * 1024,
            "Expanded update exceeds 8 GiB"
        );
        if Path::new(&name).starts_with("__MACOSX") {
            continue;
        }
        let output = destination.join(&name);
        if entry
            .unix_mode()
            .is_some_and(|mode| mode & 0o170000 == 0o120000)
        {
            ensure!(entry.size() <= 4096, "Oversized symlink");
            let mut target = String::new();
            entry.read_to_string(&mut target)?;
            validate_link(&name, &target)?;
            links.push((output, target));
        } else if entry.is_dir() {
            fs::create_dir_all(&output)?;
        } else {
            fs::create_dir_all(output.parent().context("Missing ZIP entry parent")?)?;
            let mut file = File::options().write(true).create_new(true).open(&output)?;
            let count = std::io::copy(&mut entry, &mut file)?;
            ensure!(count == entry.size(), "Truncated ZIP entry");
            let mode = entry.unix_mode().unwrap_or(0o644) & 0o755;
            file.set_permissions(fs::Permissions::from_mode(mode))?;
            file.sync_all()?;
        }
    }
    // Install links last: archive entries can never write through an earlier symlink.
    for (path, _) in &links {
        fs::create_dir_all(path.parent().context("Missing symlink parent")?)?;
    }
    for (path, target) in links {
        symlink(target, path)?;
    }
    let app = fs::canonicalize(destination.join("Stainless.app"))?;
    check_links(&app, &app)?;
    Ok(())
}

pub fn validate_entry(name: &str) -> Result<()> {
    let path = Path::new(name);
    ensure!(
        !name.contains('\\') && !name.contains('\0'),
        "Invalid ZIP path"
    );
    ensure!(
        path.components()
            .all(|part| matches!(part, Component::Normal(_))),
        "ZIP entry escapes its extraction directory"
    );
    ensure!(
        path.starts_with("Stainless.app") || path.starts_with("__MACOSX"),
        "Unexpected ZIP root"
    );
    Ok(())
}

fn validate_link(name: &str, target: &str) -> Result<()> {
    ensure!(
        name.starts_with("Stainless.app/"),
        "Metadata entries cannot contain symlinks"
    );
    let mut depth = Path::new(name)
        .parent()
        .context("Invalid symlink")?
        .components()
        .count();
    for component in Path::new(target).components() {
        match component {
            Component::Normal(_) => depth += 1,
            Component::CurDir => {}
            Component::ParentDir if depth > 1 => depth -= 1,
            _ => bail!("Update symlink escapes its app"),
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_zip_traversal_and_extra_roots() {
        for path in [
            "../Stainless.app/evil",
            "/Stainless.app/evil",
            "Stainless.app/../../evil",
            "Other.app/Contents/MacOS/Other",
            "Stainless.app\\..\\evil",
        ] {
            assert!(validate_entry(path).is_err(), "{path}");
        }
        validate_entry("Stainless.app/Contents/MacOS/Stainless").unwrap();
    }

    #[test]
    fn allows_framework_links_but_not_escaping_links() {
        validate_link(
            "Stainless.app/Contents/Frameworks/Test.framework/Versions/Current",
            "A",
        )
        .unwrap();
        validate_link(
            "Stainless.app/Contents/Frameworks/Test.framework/Test",
            "Versions/Current/Test",
        )
        .unwrap();
        assert!(validate_link("Stainless.app/Contents/escape", "../../../outside").is_err());
        assert!(validate_link("Stainless.app/Contents/escape", "/tmp/outside").is_err());
    }

    #[test]
    fn extracts_regular_files_without_writing_through_links() {
        use std::io::Write;
        let root = tempfile::tempdir().unwrap();
        let archive = root.path().join("fixture.zip");
        let mut writer = zip::ZipWriter::new(File::create(&archive).unwrap());
        let options = zip::write::SimpleFileOptions::default().unix_permissions(0o644);
        writer
            .start_file("Stainless.app/Contents/fixture.txt", options)
            .unwrap();
        writer.write_all(b"synthetic fixture").unwrap();
        writer
            .add_symlink("Stainless.app/Contents/link", "fixture.txt", options)
            .unwrap();
        writer.finish().unwrap();
        let output = root.path().join("output");
        fs::create_dir(&output).unwrap();
        unpack(&archive, &output).unwrap();
        assert_eq!(
            fs::read(output.join("Stainless.app/Contents/link")).unwrap(),
            b"synthetic fixture"
        );
    }

    #[test]
    fn rejects_entries_below_a_symlink_instead_of_following_it() {
        use std::io::Write;
        let root = tempfile::tempdir().unwrap();
        let archive = root.path().join("fixture.zip");
        let mut writer = zip::ZipWriter::new(File::create(&archive).unwrap());
        let options = zip::write::SimpleFileOptions::default();
        writer
            .add_symlink("Stainless.app/link", "Contents", options)
            .unwrap();
        writer
            .start_file("Stainless.app/link/fixture.txt", options)
            .unwrap();
        writer.write_all(b"synthetic fixture").unwrap();
        writer.finish().unwrap();
        let output = root.path().join("output");
        fs::create_dir(&output).unwrap();
        assert!(unpack(&archive, &output).is_err());
        assert!(!output.join("Stainless.app/Contents/fixture.txt").exists());
    }
}
