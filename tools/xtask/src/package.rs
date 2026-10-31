use super::{Workspace, output, run, run_output};
use anyhow::{Context, Result, ensure};
use stainless_release::{Asset, BUNDLE_ID, FEED_NAME, REPOSITORY, UpdateFeed, sha256};
use std::fs::{self, File};
use std::io::{Read, Write};
use std::os::unix::fs::symlink;
use std::path::{Path, PathBuf};
use std::process::Command;

pub(super) fn build_ui(workspace: &Workspace) -> Result<()> {
    let destination = workspace.root.join("target/native");
    fs::create_dir_all(&destination)?;
    run(workspace
        .command("xcrun")
        .args([
            "swiftc",
            "-O",
            "-swift-version",
            "6",
            "-target",
            "arm64-apple-macosx13.0",
            "-framework",
            "AppKit",
        ])
        .arg(workspace.root.join("components/updater/macos/main.swift"))
        .arg("-o")
        .arg(destination.join("Stainless Updater")))?;
    Ok(())
}

pub(super) fn package(workspace: &Workspace) -> Result<()> {
    workspace.doctor()?;
    workspace.check_source_revision()?;
    ensure!(
        workspace.check_patch_state()?,
        "Chromium patches have not been applied"
    );
    let build_path = workspace.output().join("stainless-build.json");
    let build: serde_json::Value = serde_json::from_slice(
        &fs::read(&build_path).context("Build the complete browser before packaging")?,
    )?;
    ensure!(
        build["source"] == serde_json::to_value(&workspace.manifest)?,
        "Build manifest does not match the current pinned sources/product"
    );
    ensure!(
        build["gn_args_sha256"].as_str()
            == Some(&sha256(&workspace.root.join("chromium/args.gn"))?),
        "GN configuration changed since the browser was built"
    );
    for (field, path) in [
        ("cargo_lock_sha256", workspace.root.join("Cargo.lock")),
        (
            "browser_executable_sha256",
            workspace
                .output()
                .join("Stainless.app/Contents/MacOS/Stainless"),
        ),
        ("updater_executable_sha256", workspace.updater_binary()),
        (
            "updater_ui_sha256",
            workspace.root.join("target/native/Stainless Updater"),
        ),
    ] {
        ensure!(
            build[field].as_str() == Some(&sha256(&path)?),
            "Build input/output changed since compilation: {field}"
        );
    }
    for patch in build["patches"]
        .as_array()
        .context("Missing build patch hashes")?
    {
        let path = patch["path"].as_str().context("Missing patch path")?;
        ensure!(
            workspace.manifest.patches.contains(&PathBuf::from(path)),
            "Unrecognized recorded patch"
        );
        ensure!(
            patch["sha256"].as_str() == Some(&sha256(&workspace.root.join(path))?),
            "Patch changed since browser compilation: {path}"
        );
    }
    let source = workspace.output().join("Stainless.app");
    ensure!(
        source.join("Contents/MacOS/Stainless").is_file(),
        "Full chrome target has not been built"
    );
    let destination = workspace
        .root
        .join("dist")
        .join(workspace.manifest.product.version.to_string());
    ensure!(
        !destination.exists(),
        "Release package directory already exists; preserve it or choose a new version"
    );
    fs::create_dir_all(destination.parent().context("Missing dist parent")?)?;
    let temporary = tempfile::tempdir_in(destination.parent().context("Missing dist parent")?)?;
    let work = tempfile::tempdir_in(&workspace.cache)?;
    let image_root = work.path().join("image");
    fs::create_dir(&image_root)?;
    let app = image_root.join("Stainless.app");
    run(Command::new("/usr/bin/ditto").arg(source).arg(&app))?;
    let resources = app.join("Contents/Resources");
    let notices = temporary.path().join("CHROMIUM-NOTICES.html");
    run(workspace
        .chromium_command("python3")?
        .current_dir(workspace.source())
        .args([
            "tools/licenses/licenses.py",
            "credits",
            "--target-os",
            "mac",
            "--gn-out-dir",
            "out/Stainless",
            "--gn-target",
            "//chrome:chrome",
        ])
        .arg(&notices))?;
    ensure!(
        notices.metadata()?.len() > 0,
        "Chromium license generation produced an empty notice"
    );
    fs::copy(&notices, resources.join("CHROMIUM-NOTICES.html"))?;
    fs::copy(
        workspace.source().join("LICENSE"),
        resources.join("LICENSE.chromium"),
    )?;
    fs::copy(&build_path, resources.join("stainless-build.json"))?;
    fs::copy(&build_path, temporary.path().join("stainless-build.json"))?;
    fs::copy(
        workspace.root.join("Cargo.lock"),
        temporary.path().join("Cargo.lock"),
    )?;
    rust_notices(workspace, work.path(), temporary.path(), &resources)?;
    let helper = app.join("Contents/Helpers/Stainless Updater.app");
    fs::create_dir_all(helper.join("Contents/MacOS"))?;
    fs::create_dir_all(helper.join("Contents/Resources"))?;
    fs::copy(
        workspace.root.join("target/native/Stainless Updater"),
        helper.join("Contents/MacOS/Stainless Updater"),
    )
    .context("Compile the native updater UI before packaging")?;
    fs::copy(
        workspace.updater_binary(),
        helper.join("Contents/Resources/stainless-update"),
    )
    .context("Compile the release updater before packaging")?;
    let info = serde_json::json!({
        "CFBundleIdentifier": "dev.stainless.browser.updater",
        "CFBundleExecutable": "Stainless Updater",
        "CFBundleName": "Stainless Updates",
        "CFBundlePackageType": "APPL",
        "CFBundleShortVersionString": format!("{}.{}.{}",
            workspace.manifest.product.version.major, workspace.manifest.product.version.minor,
            workspace.manifest.product.version.patch),
        "CFBundleVersion": workspace.manifest.product.build.to_string(),
        "LSMinimumSystemVersion": workspace.manifest.platform.minimum_macos,
        "LSUIElement": true,
        "NSPrincipalClass": "NSApplication",
    });
    plist::to_file_xml(helper.join("Contents/Info.plist"), &info)?;
    let mut info = plist::Value::from_file(app.join("Contents/Info.plist"))?;
    let dictionary = info
        .as_dictionary_mut()
        .context("Invalid browser Info.plist")?;
    ensure!(
        dictionary
            .get("CFBundleIdentifier")
            .and_then(plist::Value::as_string)
            == Some(BUNDLE_ID),
        "Browser was not built with the Stainless branding patch"
    );
    dictionary.insert("CrProductDirName".into(), "Stainless".into());
    dictionary.insert(
        "StainlessVersion".into(),
        workspace.manifest.product.version.to_string().into(),
    );
    dictionary.insert(
        "StainlessBuild".into(),
        workspace.manifest.product.build.into(),
    );
    dictionary.insert(
        "StainlessChannel".into(),
        workspace.manifest.product.channel.clone().into(),
    );
    dictionary.insert(
        "StainlessProfileSchema".into(),
        u64::from(workspace.manifest.product.profile_schema).into(),
    );
    plist::to_file_xml(app.join("Contents/Info.plist"), &info)?;
    verify_dependencies(workspace, &app)?;
    sign_inside_out(&app)?;
    run(Command::new("/usr/bin/codesign")
        .args(["--verify", "--deep", "--strict"])
        .arg(&app))?;
    crate::smoke::check(
        workspace,
        &app,
        &temporary.path().join("browser-smoke.json"),
    )?;
    symlink("/Applications", image_root.join("Applications"))?;
    let stem = format!(
        "Stainless-{}-macos-arm64",
        workspace.manifest.product.version
    );
    let zip = temporary.path().join(format!("{stem}.zip"));
    run(Command::new("/usr/bin/ditto")
        .args(["-c", "-k", "--sequesterRsrc", "--keepParent"])
        .arg(&app)
        .arg(&zip))?;
    let dmg = temporary.path().join(format!("{stem}.dmg"));
    run(Command::new("/usr/bin/hdiutil")
        .args([
            "create", "-quiet", "-fs", "APFS", "-format", "UDZO", "-volname",
        ])
        .arg(format!("Stainless {}", workspace.manifest.product.version))
        .arg("-srcfolder")
        .arg(&image_root)
        .arg(&dmg))?;
    let feed = UpdateFeed {
        schema_version: 1,
        product: "stainless".into(),
        release_tag: workspace.manifest.product.release_tag(),
        release: workspace.manifest.product.clone(),
        os: workspace.manifest.platform.os.clone(),
        arch: workspace.manifest.platform.arch.clone(),
        minimum_macos: workspace.manifest.platform.minimum_macos.clone(),
        chromium_revision: workspace.manifest.chromium.revision.clone(),
        chromium_version: workspace
            .manifest
            .chromium
            .version
            .clone()
            .context("Missing Chromium version")?,
        asset: Asset {
            name: format!("{stem}.zip"),
            size: zip.metadata()?.len(),
            sha256: sha256(&zip)?,
        },
    };
    feed.validate()?;
    fs::write(
        temporary.path().join(FEED_NAME),
        serde_json::to_vec_pretty(&feed)?,
    )?;
    fs::copy(
        workspace.root.join("docs/install-macos.md"),
        temporary.path().join("INSTALL.md"),
    )?;
    fs::copy(
        workspace.root.join("docs/phase-one.md"),
        temporary.path().join("PREVIEW-LIMITATIONS.md"),
    )?;
    fs::write(
        temporary.path().join("release-status.json"),
        serde_json::to_vec_pretty(&serde_json::json!({
            "schema_version": 1,
            "phase_complete": false,
            "product_version": workspace.manifest.product.version,
            "clean_host_download_install": "not_recorded",
            "installed_unsigned_n_to_n_plus_one": "not_recorded",
            "installed_failure_recovery": "not_recorded"
        }))?,
    )?;
    let mut files = fs::read_dir(temporary.path())?
        .map(|entry| entry.map(|entry| entry.path()))
        .collect::<std::io::Result<Vec<_>>>()?;
    files.sort();
    let mut checksums = File::create(temporary.path().join("SHA256SUMS"))?;
    for path in files {
        writeln!(
            checksums,
            "{}  {}",
            sha256(&path)?,
            path.file_name()
                .context("Invalid artifact name")?
                .to_string_lossy()
        )?;
    }
    checksums.sync_all()?;
    fs::rename(temporary.path(), &destination)?;
    println!(
        "Candidate package: {}\nNo clean-host or installed update gate has been claimed.",
        destination.display()
    );
    Ok(())
}

fn rust_notices(
    workspace: &Workspace,
    work: &Path,
    release: &Path,
    resources: &Path,
) -> Result<()> {
    let vendor = work.join("updater-dependency-sources");
    run_output(
        workspace
            .command("cargo")
            .current_dir(&workspace.root)
            .args(["vendor", "--locked", "--versioned-dirs"])
            .arg(&vendor),
    )?;
    let mut notice = File::create(release.join("UPDATER-NOTICES.txt"))?;
    writeln!(
        notice,
        "Rust dependency sources are supplied in updater-dependency-sources.tar.gz.\nVersions and registry checksums are recorded in Cargo.lock.\n"
    )?;
    for dependency in fs::read_dir(&vendor)? {
        let dependency = dependency?;
        let mut found = false;
        visit(&dependency.path(), &mut |path| {
            let name = path
                .file_name()
                .context("Missing notice filename")?
                .to_string_lossy()
                .to_ascii_uppercase();
            if (name.starts_with("LICENSE")
                || name.starts_with("LICENCE")
                || name.starts_with("COPYING")
                || name.starts_with("NOTICE"))
                && path.is_file()
            {
                writeln!(
                    notice,
                    "\n===== {} =====\n",
                    path.strip_prefix(&vendor)?.display()
                )?;
                std::io::copy(&mut File::open(path)?, &mut notice)?;
                writeln!(notice)?;
                found = true;
            }
            Ok(())
        })?;
        ensure!(
            found,
            "Dependency {} has no discoverable license notice; review before redistribution",
            dependency.file_name().to_string_lossy()
        );
    }
    notice.sync_all()?;
    fs::copy(
        release.join("UPDATER-NOTICES.txt"),
        resources.join("UPDATER-NOTICES.txt"),
    )?;
    run(Command::new("/usr/bin/tar")
        .args(["-czf"])
        .arg(release.join("updater-dependency-sources.tar.gz"))
        .arg("-C")
        .arg(work)
        .arg("updater-dependency-sources"))?;
    Ok(())
}

fn visit(path: &Path, callback: &mut impl FnMut(&Path) -> Result<()>) -> Result<()> {
    if path.symlink_metadata()?.file_type().is_symlink() {
        return Ok(());
    }
    if path.is_dir() {
        for entry in fs::read_dir(path)? {
            visit(&entry?.path(), callback)?;
        }
    }
    callback(path)
}

fn macho(path: &Path) -> Result<bool> {
    if !path.is_file() {
        return Ok(false);
    }
    let mut header = [0; 4];
    if File::open(path)?.read(&mut header)? != header.len() {
        return Ok(false);
    }
    Ok(matches!(
        header,
        [0xcf, 0xfa, 0xed, 0xfe]
            | [0xfe, 0xed, 0xfa, 0xcf]
            | [0xca, 0xfe, 0xba, 0xbe]
            | [0xbe, 0xba, 0xfe, 0xca]
    ))
}

fn verify_dependencies(workspace: &Workspace, app: &Path) -> Result<()> {
    visit(app, &mut |path| {
        if macho(path)? {
            let dependencies = output(workspace.command("xcrun").arg("otool").arg("-L").arg(path))?;
            for line in dependencies.lines().skip(1) {
                let dependency = line.trim().split(" (").next().unwrap_or_default();
                ensure!(
                    dependency.starts_with('@')
                        || dependency.starts_with("/System/Library/")
                        || dependency.starts_with("/usr/lib/"),
                    "Non-relocatable runtime dependency {dependency} in {}",
                    path.display()
                );
            }
        }
        Ok(())
    })
}

fn sign_inside_out(app: &Path) -> Result<()> {
    visit(app, &mut |path| {
        if macho(path)?
            || path
                .extension()
                .is_some_and(|extension| extension == "app" || extension == "framework")
        {
            run(Command::new("/usr/bin/codesign")
                .args([
                    "--force",
                    "--sign",
                    "-",
                    "--timestamp=none",
                    "--preserve-metadata=entitlements,requirements,flags",
                ])
                .arg(path))?;
        }
        Ok(())
    })
}

fn verify_checksums(directory: &Path) -> Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    for line in fs::read_to_string(directory.join("SHA256SUMS"))?.lines() {
        let (hash, name) = line.split_once("  ").context("Malformed checksum entry")?;
        ensure!(
            Path::new(name).components().count() == 1
                && matches!(
                    Path::new(name).components().next(),
                    Some(std::path::Component::Normal(_))
                ),
            "Checksum path escapes the release directory"
        );
        let path = directory.join(name);
        ensure!(
            !path.symlink_metadata()?.file_type().is_symlink(),
            "Release assets cannot be symlinks"
        );
        ensure!(
            sha256(&path)? == hash,
            "Final asset hash mismatch for {name}"
        );
        files.push(path);
    }
    ensure!(!files.is_empty(), "Empty release manifest");
    files.push(directory.join("SHA256SUMS"));
    Ok(files)
}

pub(super) fn publish(workspace: &Workspace, directory: &Path) -> Result<()> {
    let files = verify_checksums(directory)?;
    let feed: UpdateFeed = serde_json::from_slice(&fs::read(directory.join(FEED_NAME))?)?;
    feed.validate()?;
    ensure!(
        feed.release == workspace.manifest.product,
        "Package/product manifest mismatch"
    );
    let build: serde_json::Value =
        serde_json::from_slice(&fs::read(directory.join("stainless-build.json"))?)?;
    ensure!(
        build["dirty"] == false,
        "Cannot publish a build from a dirty product checkout"
    );
    let revision = build["product_revision"]
        .as_str()
        .context("Missing product source revision")?;
    ensure!(
        revision
            == output(
                Command::new("git")
                    .current_dir(&workspace.root)
                    .args(["rev-parse", "HEAD"])
            )?
            .trim(),
        "Package was built from another product revision"
    );
    workspace.security_status()?;
    let private = output(Command::new("gh").args([
        "repo",
        "view",
        REPOSITORY,
        "--json",
        "isPrivate",
        "--jq",
        ".isPrivate",
    ]))?;
    ensure!(
        private.trim() == "true",
        "Public publication requires the outstanding license/disclosure decisions"
    );
    run(workspace
        .command("cargo")
        .current_dir(&workspace.root)
        .args(["audit", "--deny", "warnings"]))?;
    let tag = feed.release_tag;
    let notes = format!(
        "Private, unsigned/unnotarized Apple Silicon macOS preview candidate.\n\n\
        Product `{}`; source `{revision}`; Chromium `{}` (`{}`).\n\n\
        Automatic unsigned updates use HTTPS, private GitHub authorization and SHA-256, not independently authenticated publisher signatures. No browsing/crash telemetry is added. \
        Gatekeeper may require a per-app exception. See INSTALL.md and PREVIEW-LIMITATIONS.md.\n\n\
        **Phase 1 is not complete until actual clean-host download/install, baseline browsing, and installed N-to-N+1/recovery evidence is recorded in #1.**",
        feed.release.version, feed.chromium_version, feed.chromium_revision
    );
    run(Command::new("gh")
        .args([
            "release",
            "create",
            &tag,
            "--repo",
            REPOSITORY,
            "--draft",
            "--prerelease",
            "--target",
            revision,
            "--title",
        ])
        .arg(format!("Stainless {}", feed.release.version))
        .args(["--notes", &notes]))?;
    run(Command::new("gh")
        .args(["release", "upload", &tag, "--repo", REPOSITORY])
        .args(files))?;
    run(Command::new("gh").args([
        "release",
        "edit",
        &tag,
        "--repo",
        REPOSITORY,
        "--draft=false",
    ]))?;
    println!("Published private preview candidate {tag}; installed release gates remain explicit.");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn refuses_tampered_release_assets() {
        let root = tempfile::tempdir().unwrap();
        fs::write(root.path().join("fixture.zip"), b"synthetic release").unwrap();
        fs::write(
            root.path().join("SHA256SUMS"),
            format!(
                "{}  fixture.zip\n",
                sha256(&root.path().join("fixture.zip")).unwrap()
            ),
        )
        .unwrap();
        verify_checksums(root.path()).unwrap();
        fs::write(root.path().join("fixture.zip"), b"corrupt").unwrap();
        assert!(verify_checksums(root.path()).is_err());
    }

    #[test]
    fn refuses_checksum_paths_outside_the_package() {
        let root = tempfile::tempdir().unwrap();
        fs::write(
            root.path().join("SHA256SUMS"),
            format!("{}  ../outside\n", "a".repeat(64)),
        )
        .unwrap();
        assert!(verify_checksums(root.path()).is_err());
    }
}
