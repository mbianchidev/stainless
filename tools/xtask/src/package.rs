use super::{Workspace, output, reapi, run, run_output};
use anyhow::{Context, Result, ensure};
use stainless_release::{Asset, BUNDLE_ID, FEED_NAME, REPOSITORY, UpdateFeed, sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File};
use std::io::{Read, Write};
use std::os::unix::fs::symlink;
use std::path::{Path, PathBuf};
use std::process::Command;

const PRODUCT_COPYRIGHT: &str = "Copyright Synctune Open. All rights reserved.";
const STATUS_ICON: &str = "stainless-status.png";

// The updater's 18-point menu-bar item, rendered at Retina size as a one-color template.
fn render_status_icon(workspace: &Workspace, destination: &Path) -> Result<()> {
    run(
        Command::new(workspace.root.join("target/native/render-icon"))
            .arg(workspace.root.join("chromium/branding/stainless-logo.png"))
            .arg(destination)
            .args(["36", "status-template"]),
    )
}

fn build_icon_tools(workspace: &Workspace) -> Result<()> {
    let destination = workspace.root.join("target/native");
    fs::create_dir_all(&destination)?;
    for (source, binary) in [
        ("tools/xtask/macos/render_icon.swift", "render-icon"),
        (
            "tools/xtask/macos/render_dmg_background.swift",
            "render-dmg-background",
        ),
        ("tests/icon_tests.swift", "icon-tests"),
    ] {
        run(workspace
            .command("xcrun")
            .args([
                "swiftc",
                "-O",
                "-parse-as-library",
                "-swift-version",
                "6",
                "-warnings-as-errors",
            ])
            .arg(workspace.root.join("tools/xtask/macos/icon.swift"))
            .arg(workspace.root.join(source))
            .arg("-o")
            .arg(destination.join(binary)))?;
    }
    run(Command::new(destination.join("icon-tests"))
        .arg(workspace.root.join("chromium/branding/stainless-logo.png"))
        .arg(workspace.root.join("chromium/branding/stainless-icon.png")))?;
    Ok(())
}

pub(super) fn build_ui(workspace: &Workspace, profile_runtime: &Path) -> Result<()> {
    build_icon_tools(workspace)?;
    let destination = workspace.root.join("target/native");
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
        .arg(
            workspace
                .root
                .join("components/updater/macos/branding.swift"),
        )
        .arg(
            workspace
                .root
                .join("components/updater/macos/github_token_template.swift"),
        )
        .arg(
            workspace
                .root
                .join("components/updater/macos/managed_profiles.swift"),
        )
        .arg("-o")
        .arg(destination.join("Stainless Updater")))?;
    let branding_tests = destination.join("updater-branding-tests");
    run(workspace
        .command("xcrun")
        .args([
            "swiftc",
            "-O",
            "-swift-version",
            "6",
            "-warnings-as-errors",
            "-parse-as-library",
            "-target",
            "arm64-apple-macosx13.0",
            "-framework",
            "AppKit",
        ])
        .arg(
            workspace
                .root
                .join("components/updater/macos/branding.swift"),
        )
        .arg(
            workspace
                .root
                .join("components/updater/tests/branding_tests.swift"),
        )
        .arg("-o")
        .arg(&branding_tests))?;
    let status_icon = destination.join("updater-status-template.png");
    render_status_icon(workspace, &status_icon)?;
    run(Command::new(branding_tests)
        .arg(workspace.root.join("chromium/branding/stainless-icon.png"))
        .arg(&status_icon)
        .arg(destination.join("updater-branding-fixture.png")))?;
    let tests = destination.join("github-token-template-tests");
    run(workspace
        .command("xcrun")
        .args([
            "swiftc",
            "-O",
            "-swift-version",
            "6",
            "-warnings-as-errors",
            "-parse-as-library",
            "-target",
            "arm64-apple-macosx13.0",
        ])
        .arg(
            workspace
                .root
                .join("components/updater/macos/github_token_template.swift"),
        )
        .arg(
            workspace
                .root
                .join("components/updater/tests/github_token_template_tests.swift"),
        )
        .arg("-o")
        .arg(&tests))?;
    run(&mut Command::new(tests))?;
    let profile_tests = destination.join("managed-profiles-tests");
    run(workspace
        .command("xcrun")
        .args([
            "swiftc",
            "-O",
            "-swift-version",
            "6",
            "-warnings-as-errors",
            "-parse-as-library",
            "-target",
            "arm64-apple-macosx13.0",
            "-framework",
            "AppKit",
        ])
        .arg(
            workspace
                .root
                .join("components/updater/macos/managed_profiles.swift"),
        )
        .arg(
            workspace
                .root
                .join("components/updater/tests/managed_profiles_tests.swift"),
        )
        .arg("-o")
        .arg(&profile_tests))?;
    run(Command::new(&profile_tests)
        .arg("--snapshot")
        .arg(destination.join("managed-profiles-fixture.png"))
        .arg("--recovery-snapshot")
        .arg(destination.join("managed-recovery-fixture.png")))?;
    let home = tempfile::Builder::new()
        .prefix("stainless-native-recovery-home-")
        .tempdir()?;
    run(Command::new(profile_tests)
        .arg("--production-helper")
        .arg(profile_runtime)
        .env("HOME", home.path())
        .env("PATH", home.path().join("no-developer-tools")))?;
    Ok(())
}

pub(super) fn package(workspace: &Workspace) -> Result<()> {
    reapi::reject_cache_for("package", workspace.reapi.as_ref())?;
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
    reapi::require_uncached(&workspace.output(), Some(&build))?;
    ensure!(
        build["source"] == serde_json::to_value(&workspace.manifest)?,
        "Build manifest does not match the current pinned sources/product"
    );
    ensure!(
        build["profile_chooser_version"] == stainless_profiles::native::PROTOCOL_VERSION,
        "Build manifest does not record this native profile runtime"
    );
    ensure!(
        build["profile_recovery_version"] == stainless_profiles::native::RECOVERY_VERSION,
        "Build manifest does not record this native profile recovery runtime"
    );
    ensure!(
        build["profile_safe_mode_version"] == stainless_profiles::native::SAFE_MODE_VERSION
            && build["managed_safe_mode_version"]
                == stainless_profiles::app::MANAGED_SAFE_MODE_VERSION,
        "Build manifest does not record a compatible native/browser Safe Mode pair"
    );
    stainless_profiles::app::require_managed_safe_mode(&workspace.output().join("Stainless.app"))?;
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
        (
            "branding_logo_sha256",
            workspace.root.join("chromium/branding/stainless-logo.png"),
        ),
        (
            "branding_icon_sha256",
            workspace.root.join("chromium/branding/stainless-icon.png"),
        ),
        (
            "branding_renderer_sha256",
            workspace.root.join("target/native/render-icon"),
        ),
        (
            "dmg_renderer_sha256",
            workspace.root.join("target/native/render-dmg-background"),
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
    let icon = build_app_icon(workspace, work.path())?;
    fs::copy(&icon, resources.join("stainless.icns"))?;
    fs::copy(
        workspace.root.join("chromium/branding/stainless-logo.png"),
        temporary.path().join("stainless-logo.png"),
    )?;
    fs::copy(
        workspace.root.join("chromium/branding/stainless-icon.png"),
        temporary.path().join("stainless-icon.png"),
    )?;
    let notices = temporary.path().join("CHROMIUM-NOTICES.html");
    run(workspace
        .chromium_command("python-bin/python3")?
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
    fs::copy(&icon, helper.join("Contents/Resources/stainless.icns"))?;
    render_status_icon(
        workspace,
        &helper.join("Contents/Resources").join(STATUS_ICON),
    )?;
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
        "CFBundleIconFile": "stainless.icns",
        "StainlessStatusIconFile": STATUS_ICON,
        "NSHumanReadableCopyright": PRODUCT_COPYRIGHT,
        "StainlessUpdateRepository": REPOSITORY,
        "StainlessProfileChooserVersion": stainless_profiles::native::PROTOCOL_VERSION,
        "StainlessProfileRecoveryVersion": stainless_profiles::native::RECOVERY_VERSION,
        "StainlessProfileSafeModeVersion": stainless_profiles::native::SAFE_MODE_VERSION,
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
    dictionary.insert("CFBundleIconFile".into(), "stainless.icns".into());
    dictionary.insert("NSHumanReadableCopyright".into(), PRODUCT_COPYRIGHT.into());
    dictionary.remove("CFBundleIconName");
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
    let volume_name = format!("Stainless {}", workspace.manifest.product.version);
    let background = image_root.join(".background");
    fs::create_dir(&background)?;
    run(
        Command::new(workspace.root.join("target/native/render-dmg-background"))
            .arg(background.join("background.png")),
    )?;
    run(workspace
        .chromium_command("python-bin/python3")?
        .arg("-B")
        .arg(workspace.root.join("tools/xtask/macos/dmg_layout.py"))
        .arg(workspace.source())
        .arg(&image_root)
        .arg(&volume_name))?;
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
    create_disk_image(workspace, &image_root, &icon, &volume_name, &dmg)?;
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
    fs::copy(
        workspace.root.join("docs/degoogling.md"),
        temporary.path().join("degoogling.md"),
    )?;
    fs::copy(
        workspace.root.join("docs/profile-isolation.md"),
        temporary.path().join("profile-isolation.md"),
    )?;
    fs::copy(
        workspace.root.join("LICENSE"),
        temporary.path().join("LICENSE"),
    )
    .context("Cannot copy the Stainless LICENSE into the package")?;
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

fn build_app_icon(workspace: &Workspace, work: &Path) -> Result<PathBuf> {
    let rendered = workspace.root.join("chromium/branding/stainless-icon.png");
    let iconset = work.join("Stainless.iconset");
    fs::create_dir(&iconset)?;
    for (name, pixels) in [
        ("icon_16x16.png", 16),
        ("icon_16x16@2x.png", 32),
        ("icon_32x32.png", 32),
        ("icon_32x32@2x.png", 64),
        ("icon_128x128.png", 128),
        ("icon_128x128@2x.png", 256),
        ("icon_256x256.png", 256),
        ("icon_256x256@2x.png", 512),
        ("icon_512x512.png", 512),
        ("icon_512x512@2x.png", 1024),
    ] {
        run(Command::new("/usr/bin/sips")
            .arg("-z")
            .args([pixels.to_string(), pixels.to_string()])
            .arg(&rendered)
            .arg("--out")
            .arg(iconset.join(name)))?;
    }
    let icon = work.join("Stainless.icns");
    run(Command::new("/usr/bin/iconutil")
        .args(["-c", "icns", "-o"])
        .arg(&icon)
        .arg(iconset))?;
    Ok(icon)
}

fn create_disk_image(
    workspace: &Workspace,
    image_root: &Path,
    icon: &Path,
    volume_name: &str,
    dmg: &Path,
) -> Result<()> {
    fs::copy(icon, image_root.join(".VolumeIcon.icns"))?;
    fs::create_dir_all(&workspace.cache)?;
    let staging = tempfile::Builder::new()
        .prefix("stainless-dmg-")
        .tempdir_in(&workspace.cache)?;
    let writable = staging.path().join("image.dmg");
    run(Command::new("/usr/bin/hdiutil")
        .args([
            "create", "-quiet", "-fs", "APFS", "-format", "UDRW", "-volname",
        ])
        .arg(volume_name)
        .arg("-srcfolder")
        .arg(image_root)
        .arg(&writable))?;
    let mut mounted = MountedDiskImage::attach(staging, false)?;
    run(workspace
        .command("xcrun")
        .args(["SetFile", "-a", "C"])
        .arg(&mounted.root))?;
    mounted.detach()?;
    run(Command::new("/usr/bin/hdiutil")
        .args(["convert", "-quiet", "-format", "UDZO"])
        .arg(&mounted.image)
        .arg("-o")
        .arg(dmg))?;
    archive_branded_dmg(workspace, dmg, icon)?;
    Ok(())
}

struct MountedDiskImage {
    staging: Option<tempfile::TempDir>,
    image: PathBuf,
    root: PathBuf,
    attached: bool,
}

impl MountedDiskImage {
    fn attach(staging: tempfile::TempDir, read_only: bool) -> Result<Self> {
        let image = staging.path().join("image.dmg");
        let root = staging.path().join("mounted");
        fs::create_dir(&root)?;
        let mounted = Self {
            staging: Some(staging),
            image,
            root,
            attached: true,
        };
        let mut command = Command::new("/usr/bin/hdiutil");
        command.args(["attach", "-quiet", "-nobrowse", "-noautoopen"]);
        if read_only {
            command.arg("-readonly");
        }
        run(command
            .arg("-mountpoint")
            .arg(&mounted.root)
            .arg(&mounted.image))?;
        Ok(mounted)
    }

    fn detach(&mut self) -> Result<()> {
        if !self.attached {
            return Ok(());
        }
        let result = run(Command::new("/usr/bin/hdiutil")
            .args(["detach", "-quiet"])
            .arg(&self.root));
        self.attached = false;
        if result.is_err()
            && let Some(staging) = self.staging.take()
        {
            let retained = staging.keep();
            return result.with_context(|| {
                format!(
                    "Owned image could not be detached; preserve its mounted staging at {}",
                    retained.display()
                )
            });
        }
        result
    }
}

impl Drop for MountedDiskImage {
    fn drop(&mut self) {
        if let Err(error) = self.detach() {
            eprintln!("Disk-image cleanup failed: {error:#}");
        }
    }
}

fn archive_branded_dmg(workspace: &Workspace, dmg: &Path, icon: &Path) -> Result<PathBuf> {
    let bytes = fs::read(icon).context("Cannot read the Stainless DMG icon")?;
    ensure!(
        bytes.len() >= 8 && &bytes[..4] == b"icns",
        "Invalid Stainless DMG icon header"
    );
    ensure!(
        u32::from_be_bytes(bytes[4..8].try_into()?) as usize == bytes.len(),
        "Invalid Stainless DMG icon length"
    );
    let mut resource = tempfile::Builder::new()
        .prefix("stainless-dmg-icon-")
        .suffix(".r")
        .tempfile_in(dmg.parent().context("DMG has no output directory")?)?;
    {
        let mut writer = std::io::BufWriter::new(resource.as_file_mut());
        writeln!(writer, "data 'icns' (-16455) {{")?;
        for chunk in bytes.chunks(32) {
            write!(writer, "    $\"")?;
            for byte in chunk {
                write!(writer, "{byte:02X}")?;
            }
            writeln!(writer, "\"")?;
        }
        writeln!(writer, "}};")?;
        writer.flush()?;
    }
    run(workspace
        .command("xcrun")
        .args(["Rez", "-a"])
        .arg(resource.path())
        .arg("-o")
        .arg(dmg))?;
    run(workspace
        .command("xcrun")
        .args(["SetFile", "-a", "C"])
        .arg(dmg))?;
    let archive = dmg.with_extension("dmg.zip");
    run(Command::new("/usr/bin/ditto")
        .args([
            "-c",
            "-k",
            "--rsrc",
            "--extattr",
            "--sequesterRsrc",
            "--zlibCompressionLevel",
            "0",
        ])
        .arg(dmg)
        .arg(&archive))?;
    Ok(archive)
}

fn rust_notices(
    workspace: &Workspace,
    work: &Path,
    release: &Path,
    resources: &Path,
) -> Result<()> {
    let metadata: serde_json::Value = serde_json::from_slice(
        &run_output(
            workspace
                .command("cargo")
                .current_dir(&workspace.root)
                .args([
                    "metadata",
                    "--locked",
                    "--format-version",
                    "1",
                    "--filter-platform",
                    "aarch64-apple-darwin",
                ]),
        )?
        .stdout,
    )?;
    let shipped = macos_updater_dependencies(&metadata)?;
    let all_sources = work.join("vendor-all-targets");
    let vendor = work.join("updater-dependency-sources");
    run_output(
        workspace
            .command("cargo")
            .current_dir(&workspace.root)
            .args(["vendor", "--locked", "--versioned-dirs"])
            .arg(&all_sources),
    )?;
    fs::create_dir(&vendor)?;
    for package in shipped {
        let source = all_sources.join(&package);
        ensure!(
            source.is_dir(),
            "Missing vendored source for shipped dependency {package}"
        );
        run(Command::new("/usr/bin/ditto")
            .arg(source)
            .arg(vendor.join(package)))?;
    }
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
    let sysroot =
        PathBuf::from(output(workspace.command("rustc").args(["--print", "sysroot"]))?.trim());
    for (name, source) in rust_runtime_notices(&sysroot)? {
        fs::copy(&source, release.join(name))?;
        fs::copy(source, resources.join(name))?;
    }
    run(Command::new("/usr/bin/tar")
        .args(["-czf"])
        .arg(release.join("updater-dependency-sources.tar.gz"))
        .arg("-C")
        .arg(work)
        .arg("updater-dependency-sources"))?;
    Ok(())
}

fn macos_updater_dependencies(metadata: &serde_json::Value) -> Result<BTreeSet<String>> {
    let packages: BTreeMap<_, _> = metadata["packages"]
        .as_array()
        .context("Missing Cargo packages")?
        .iter()
        .map(|package| {
            Ok((
                package["id"].as_str().context("Missing Cargo package ID")?,
                package,
            ))
        })
        .collect::<Result<_>>()?;
    let nodes: BTreeMap<_, _> = metadata["resolve"]["nodes"]
        .as_array()
        .context("Missing Cargo dependency graph")?
        .iter()
        .map(|node| Ok((node["id"].as_str().context("Missing Cargo node ID")?, node)))
        .collect::<Result<_>>()?;
    let root = packages
        .iter()
        .find(|(_, package)| {
            package["name"] == "stainless-updater"
                && package
                    .get("source")
                    .is_some_and(serde_json::Value::is_null)
        })
        .map(|(id, _)| *id)
        .context("Updater is missing from Cargo metadata")?;
    let mut pending = vec![root];
    let mut visited = BTreeSet::new();
    let mut sources = BTreeMap::new();
    while let Some(id) = pending.pop() {
        if !visited.insert(id) {
            continue;
        }
        let package = packages
            .get(id)
            .context("Dependency is missing its package metadata")?;
        let source = package
            .get("source")
            .context("Missing Cargo package source")?;
        if !source.is_null() {
            ensure!(source.as_str().is_some(), "Invalid Cargo package source");
            let directory = format!(
                "{}-{}",
                package["name"]
                    .as_str()
                    .context("Missing Cargo package name")?,
                package["version"]
                    .as_str()
                    .context("Missing Cargo package version")?
            );
            ensure!(
                Path::new(&directory).components().count() == 1
                    && matches!(
                        Path::new(&directory).components().next(),
                        Some(std::path::Component::Normal(_))
                    ),
                "Cargo dependency directory escapes the vendor root"
            );
            ensure!(
                sources.insert(directory, id).is_none(),
                "Multiple Cargo sources share a package/version; explicit vendor mapping is required"
            );
        }
        let node = nodes
            .get(id)
            .context("Dependency is missing its resolved graph node")?;
        for dependency in node["deps"]
            .as_array()
            .context("Missing Cargo dependency edges")?
        {
            let kinds = dependency["dep_kinds"]
                .as_array()
                .context("Missing Cargo dependency kinds")?;
            ensure!(!kinds.is_empty(), "Cargo dependency has no resolved kind");
            let mut include = false;
            for kind in kinds {
                let kind = kind.get("kind").context("Missing Cargo dependency kind")?;
                match kind.as_str() {
                    None if kind.is_null() => include = true,
                    Some("build") => include = true,
                    Some("dev") => {}
                    _ => anyhow::bail!("Unsupported Cargo dependency kind"),
                }
            }
            if include {
                pending.push(
                    dependency["pkg"]
                        .as_str()
                        .context("Missing dependency package ID")?,
                );
            }
        }
    }
    Ok(sources.into_keys().collect())
}

fn rust_runtime_notices(sysroot: &Path) -> Result<Vec<(&'static str, PathBuf)>> {
    [
        ("RUST-STDLIB-NOTICES.html", ["share/doc/rustc/COPYRIGHT-library.html", "share/doc/rust/COPYRIGHT-library.html"]),
        ("RUST-LICENSE-MIT", ["LICENSE-MIT", "share/doc/cargo/LICENSE-MIT"]),
        ("RUST-LICENSE-APACHE", ["LICENSE-APACHE", "share/doc/cargo/LICENSE-APACHE"]),
    ].into_iter().map(|(name, candidates)| {
        let source = candidates.into_iter().map(|path| sysroot.join(path)).find(|path| path.is_file())
            .with_context(|| format!("Pinned Rust toolchain is missing {name}; restore its distribution notices before packaging"))?;
        Ok((name, source))
    }).collect()
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

pub(super) fn verify_dependencies(workspace: &Workspace, app: &Path) -> Result<()> {
    visit(app, &mut |path| {
        if macho(path)? {
            let commands = output(
                workspace
                    .command(
                        workspace
                            .source()
                            .join("third_party/llvm-build/Release+Asserts/bin/llvm-objdump"),
                    )
                    .arg("--private-headers")
                    .arg(path),
            )?;
            ensure!(
                commands.contains("file format mach-o"),
                "Pinned LLVM did not decode a Mach-O binary: {}",
                path.display()
            );
            validate_macho_dependencies(&commands)
                .with_context(|| format!("Invalid runtime dependencies in {}", path.display()))?;
        }
        Ok(())
    })
}

fn validate_macho_dependencies(commands: &str) -> Result<()> {
    let mut needs_name = false;
    for line in commands.lines().map(str::trim) {
        if let Some(command) = line.strip_prefix("cmd ") {
            ensure!(
                !needs_name,
                "Dynamic library load command is missing its path"
            );
            needs_name = matches!(
                command,
                "LC_LOAD_DYLIB"
                    | "LC_LOAD_WEAK_DYLIB"
                    | "LC_REEXPORT_DYLIB"
                    | "LC_LAZY_LOAD_DYLIB"
                    | "LC_LOAD_UPWARD_DYLIB"
            );
        } else if needs_name && let Some(name) = line.strip_prefix("name ") {
            let (dependency, _) = name
                .rsplit_once(" (offset ")
                .context("Malformed dynamic library load path")?;
            ensure!(
                dependency.starts_with('@')
                    || dependency.starts_with("/System/Library/")
                    || dependency.starts_with("/usr/lib/"),
                "Non-relocatable runtime dependency {dependency}"
            );
            needs_name = false;
        }
    }
    ensure!(
        !needs_name,
        "Dynamic library load command is missing its path"
    );
    Ok(())
}

pub(super) fn sign_inside_out(app: &Path) -> Result<()> {
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

fn verify_release_policy(root: &Path, directory: &Path, files: &[PathBuf]) -> Result<()> {
    let license = directory.join("LICENSE");
    ensure!(
        files.contains(&license),
        "Package lacks the Stainless LICENSE; repackage from a licensed source revision"
    );
    ensure!(
        sha256(&license)?
            == sha256(&root.join("LICENSE")).context("Missing the source revision's LICENSE")?,
        "Packaged LICENSE differs from the source revision's LICENSE"
    );
    ensure!(
        root.join("SECURITY.md").is_file(),
        "Publication requires the SECURITY.md vulnerability-disclosure policy"
    );
    Ok(())
}

fn require_only_listed_assets(directory: &Path, files: &[PathBuf]) -> Result<()> {
    let listed: BTreeSet<_> = files.iter().filter_map(|path| path.file_name()).collect();
    for entry in fs::read_dir(directory)? {
        let name = entry?.file_name();
        ensure!(
            listed.contains(name.as_os_str()),
            "Release contains an asset missing from SHA256SUMS: {}",
            name.to_string_lossy()
        );
    }
    Ok(())
}

struct VerifiedPackage {
    feed: UpdateFeed,
    revision: String,
    files: Vec<PathBuf>,
}

fn verify_package(workspace: &Workspace, directory: &Path) -> Result<VerifiedPackage> {
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
        .context("Missing product source revision")?
        .to_owned();
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
    verify_release_policy(&workspace.root, directory, &files)?;
    workspace.security_status()?;
    run(workspace
        .command("cargo")
        .current_dir(&workspace.root)
        .args(["audit", "--deny", "warnings"]))?;
    Ok(VerifiedPackage {
        feed,
        revision,
        files,
    })
}

/// Release CI runs this on the downloaded draft assets before publishing them.
pub(super) fn verify_release(workspace: &Workspace, directory: &Path) -> Result<()> {
    let package = verify_package(workspace, directory)?;
    require_only_listed_assets(directory, &package.files)?;
    println!(
        "Verified {} for source {}: {} assets, current Chromium stable and dependency advisories",
        package.feed.release_tag,
        package.revision,
        package.files.len()
    );
    Ok(())
}

pub(super) fn publish(workspace: &Workspace, directory: &Path) -> Result<()> {
    let VerifiedPackage {
        feed,
        revision,
        files,
    } = verify_package(workspace, directory)?;
    let revision = revision.as_str();
    let tag = feed.release_tag;
    let notes = format!(
        "Unsigned/unnotarized Apple Silicon macOS preview candidate.\n\n\
        Product `{}`; source `{revision}`; Chromium `{}` (`{}`). Built and packaged on the maintainer's build Mac; \
        release CI re-verified the SHA256SUMS, source tag, license/security policy, current Chromium stable and \
        dependency advisories before publishing.\n\n\
        Stainless source is licensed under Apache-2.0 (attached LICENSE). Chromium and other bundled components keep their own licenses; see the attached notices. \
        Report vulnerabilities privately as described in SECURITY.md, not in public issues.\n\n\
        Stainless product branding and a black icon retain the original S colors. Built-in Google browser services are unavailable. \
        Use the additional `.dmg.zip` download and macOS Archive Utility to retain both the DMG file and mounted-volume icons; the plain `.dmg` remains available. \
        Google Safe Browsing is replaced by on-device matching against downloaded HaGeZi TIF mini and CERT Polska domain lists (on by default, no per-site lookups). \
        Opt-in password breach checks use Have I Been Pwned k-anonymity ranges. Download reputation has no replacement. See the attached degoogling.md. \
        Ordinary TLS validation, site isolation and renderer sandboxing remain required.\n\n\
        Automatic unsigned updates use HTTPS, anonymous public GitHub release metadata (an optional read-only token only raises rate limits) and SHA-256, not independently authenticated publisher signatures. No browsing/crash telemetry is added. \
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
    println!(
        "Staged draft release {tag} for source {revision}.\n\
        Push the annotated tag {tag} at that commit; release CI verifies the draft and publishes it \
        (see docs/releasing.md). Installed release gates remain explicit."
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[cfg(target_os = "macos")]
    fn native_stripping_preserves_execution_signing_and_failed_inputs() -> Result<()> {
        let workspace = Workspace::load()?;
        let directory = tempfile::tempdir()?;
        let source = directory.path().join("fixture.c");
        let executable = directory.path().join("fixture");
        fs::write(
            &source,
            "#include <stdio.h>\n\
             __attribute__((noinline)) static int fictional_private_helper(void) { return 23; }\n\
             int main(void) { puts(fictional_private_helper() == 23 ? \"fictional-strip-ok\" : \"wrong\"); return 0; }\n",
        )?;
        run(workspace
            .command("xcrun")
            .args(["clang", "-g", "-O0"])
            .arg(&source)
            .arg("-o")
            .arg(&executable))?;
        let before = output(Command::new("/usr/bin/nm").arg("-a").arg(&executable))?;
        ensure!(before.contains("_fictional_private_helper"));
        let expected = run_output(&mut Command::new(&executable))?.stdout;
        let script = workspace.root.join("tools/xtask/macos/strip_rust.sh");
        run(Command::new("/bin/bash").arg(&script).arg(&executable))?;
        let after = output(Command::new("/usr/bin/nm").arg("-a").arg(&executable))?;
        ensure!(!after.contains("_fictional_private_helper"));
        ensure!(run_output(&mut Command::new(&executable))?.stdout == expected);
        run(Command::new("/usr/bin/codesign")
            .args(["--verify", "--strict"])
            .arg(&executable))?;
        let invalid = directory.path().join("not-mach-o");
        fs::write(&invalid, b"fictional input must survive")?;
        let failure = Command::new("/bin/bash")
            .arg(&script)
            .arg(&invalid)
            .output()?;
        ensure!(!failure.status.success());
        ensure!(fs::read(invalid)? == b"fictional input must survive");
        ensure!(fs::read_dir(directory.path())?.all(|entry| {
            entry.is_ok_and(|entry| {
                !entry
                    .file_name()
                    .to_string_lossy()
                    .contains(".native-strip.")
            })
        }));
        Ok(())
    }

    #[test]
    #[cfg(target_os = "macos")]
    fn custom_logo_produces_a_native_icon_with_a_1024_pixel_retina_frame() -> Result<()> {
        let workspace = Workspace::load()?;
        build_icon_tools(&workspace)?;
        let temporary = tempfile::tempdir()?;
        let icon = build_app_icon(&workspace, temporary.path())?;
        let bytes = fs::read(&icon)?;
        ensure!(
            bytes.len() >= 8 && &bytes[..4] == b"icns",
            "Invalid native icon header"
        );
        ensure!(
            u32::from_be_bytes(bytes[4..8].try_into()?) as usize == bytes.len(),
            "Native icon length does not match its header"
        );
        let decoded = temporary.path().join("decoded.iconset");
        run(Command::new("/usr/bin/iconutil")
            .args(["-c", "iconset", "-o"])
            .arg(&decoded)
            .arg(&icon))?;
        let frame = output(
            Command::new("/usr/bin/sips")
                .args(["-g", "pixelWidth", "-g", "pixelHeight", "-g", "hasAlpha"])
                .arg(decoded.join("icon_512x512@2x.png")),
        )?;
        ensure!(
            frame.contains("pixelWidth: 1024")
                && frame.contains("pixelHeight: 1024")
                && frame.contains("hasAlpha: yes"),
            "Native retina frame lost its size or transparency"
        );
        Ok(())
    }

    #[cfg(target_os = "macos")]
    fn has_custom_icon(workspace: &Workspace, path: &Path) -> Result<bool> {
        Ok(output(
            workspace
                .command("xcrun")
                .args(["GetFileInfo", "-a"])
                .arg(path),
        )?
        .contains('C'))
    }

    #[test]
    #[cfg(target_os = "macos")]
    fn dmg_file_icon_survives_a_metadata_preserving_download() -> Result<()> {
        let workspace = Workspace::load()?;
        let temporary = tempfile::tempdir()?;
        let icon = build_app_icon(&workspace, temporary.path())?;
        let dmg = temporary.path().join("Stainless Fictional.dmg");
        let payload = b"fictional disk-image payload";
        fs::write(&dmg, payload)?;
        let archive = archive_branded_dmg(&workspace, &dmg, &icon)?;
        ensure!(fs::read(&dmg)? == payload);
        ensure!(has_custom_icon(&workspace, &dmg)?);
        let parsed = run_output(
            workspace
                .command("xcrun")
                .args(["DeRez", "-only", "icns"])
                .arg(&dmg),
        )?
        .stdout;
        let resource_id = b"data 'icns' (-16455)";
        ensure!(
            parsed
                .windows(resource_id.len())
                .any(|bytes| bytes == resource_id),
            "DMG is missing the native Finder custom-icon resource"
        );
        let resource = fs::read(dmg.join("..namedfork/rsrc"))?;
        let icon_bytes = fs::read(&icon)?;
        ensure!(
            resource
                .windows(icon_bytes.len())
                .any(|bytes| bytes == icon_bytes),
            "DMG resource fork does not contain its Stainless icon"
        );
        let downloaded = temporary.path().join("downloaded.dmg.zip");
        fs::write(&downloaded, fs::read(archive)?)?;
        let members = output(
            Command::new("/usr/bin/unzip")
                .args(["-Z", "-1"])
                .arg(&downloaded),
        )?;
        ensure!(
            members
                .lines()
                .any(|name| name == "Stainless Fictional.dmg"),
            "DMG download archive has unexpected paths: {members}"
        );
        let restored = temporary.path().join("restored");
        run(Command::new("/usr/bin/ditto")
            .args(["-x", "-k", "--rsrc", "--extattr"])
            .arg(downloaded)
            .arg(&restored))?;
        let restored_dmg = restored.join("Stainless Fictional.dmg");
        ensure!(fs::read(&restored_dmg)? == payload);
        ensure!(has_custom_icon(&workspace, &restored_dmg)?);
        ensure!(fs::read(restored_dmg.join("..namedfork/rsrc"))? == resource);
        Ok(())
    }

    #[test]
    #[cfg(target_os = "macos")]
    fn dmg_file_rejects_invalid_icon_without_changing_the_image() -> Result<()> {
        let workspace = Workspace::load()?;
        let temporary = tempfile::tempdir()?;
        let dmg = temporary.path().join("Stainless Fictional.dmg");
        let icon = temporary.path().join("invalid.icns");
        let payload = b"fictional disk-image payload";
        fs::write(&dmg, payload)?;
        for bytes in [
            b"not an icon".as_slice(),
            b"icns\x00\x00\x00\xff".as_slice(),
        ] {
            fs::write(&icon, bytes)?;
            ensure!(
                archive_branded_dmg(&workspace, &dmg, &icon).is_err(),
                "Invalid branding must not produce an apparently branded DMG"
            );
            ensure!(fs::read(&dmg)? == payload);
            ensure!(!has_custom_icon(&workspace, &dmg)?);
            ensure!(!dmg.with_extension("dmg.zip").exists());
        }
        Ok(())
    }

    #[test]
    #[cfg(target_os = "macos")]
    #[ignore = "mounts an owned fixture image; requires a dedicated macOS test host"]
    fn dmg_volume_icon_survives_compression() -> Result<()> {
        let workspace = Workspace::load()?;
        let temporary = tempfile::tempdir()?;
        let icon = build_app_icon(&workspace, temporary.path())?;
        let image_root = temporary.path().join("image");
        fs::create_dir(&image_root)?;
        fs::write(
            image_root.join("fixture.txt"),
            b"fictional installer fixture",
        )?;
        symlink("/Applications", image_root.join("Applications"))?;
        let dmg = temporary.path().join("Stainless Fictional.dmg");
        create_disk_image(&workspace, &image_root, &icon, "Stainless Fictional", &dmg)?;
        run(Command::new("/usr/bin/hdiutil")
            .args(["verify", "-quiet"])
            .arg(&dmg))?;
        let download = tempfile::Builder::new()
            .prefix("stainless-dmg-download-")
            .tempdir_in(&workspace.cache)?;
        let downloaded = download.path().join("image.dmg");
        fs::write(&downloaded, fs::read(&dmg)?)?;
        ensure!(!has_custom_icon(&workspace, &downloaded)?);
        let mounted = MountedDiskImage::attach(download, true)?;
        ensure!(
            fs::read(mounted.root.join(".VolumeIcon.icns"))? == fs::read(&icon)?,
            "Compressed DMG lost the Stainless volume icon"
        );
        ensure!(
            has_custom_icon(&workspace, &mounted.root)?,
            "Compressed DMG volume is missing its custom-icon flag"
        );
        ensure!(fs::read_link(mounted.root.join("Applications"))? == Path::new("/Applications"));
        ensure!(fs::read(mounted.root.join("fixture.txt"))? == b"fictional installer fixture");
        let staging = mounted
            .root
            .parent()
            .context("Mounted fixture has no staging directory")?
            .to_path_buf();
        drop(mounted);
        ensure!(
            !staging.exists(),
            "Owned disk-image staging remains after automatic detach: {}",
            staging.display()
        );
        Ok(())
    }

    #[test]
    fn distinguishes_a_dylibs_install_id_from_its_loaded_dependencies() {
        let commands = "Load command 1\ncmd LC_ID_DYLIB\nname ./libvulkan.dylib (offset 24)\n\
            Load command 2\ncmd LC_LOAD_DYLIB\nname /usr/lib/libSystem.B.dylib (offset 24)\n\
            Load command 3\ncmd LC_LOAD_WEAK_DYLIB\nname @rpath/Synthetic.framework/Synthetic (offset 24)\n";
        validate_macho_dependencies(commands).unwrap();
        assert!(
            validate_macho_dependencies(&commands.replace(
                "@rpath/Synthetic.framework/Synthetic",
                "/opt/synthetic/lib.dylib"
            ))
            .is_err()
        );
        assert!(validate_macho_dependencies("cmd LC_LOAD_DYLIB\ncmd LC_CODE_SIGNATURE\n").is_err());
    }

    #[test]
    fn validates_all_macho_dynamic_library_load_commands() {
        for command in [
            "LC_LOAD_DYLIB",
            "LC_LOAD_WEAK_DYLIB",
            "LC_REEXPORT_DYLIB",
            "LC_LAZY_LOAD_DYLIB",
            "LC_LOAD_UPWARD_DYLIB",
        ] {
            assert!(
                validate_macho_dependencies(&format!(
                    "cmd {command}\nname /synthetic/build/lib.dylib (offset 24)\n"
                ))
                .is_err()
            );
        }
    }

    #[test]
    fn packages_only_the_updaters_target_and_non_dev_dependency_closure() {
        let metadata = serde_json::json!({
            "packages": [
                {"id":"root","name":"stainless-updater","version":"0.1.0","source":null},
                {"id":"runtime","name":"synthetic-runtime","version":"1.0.0","source":"registry+https://example.invalid"},
                {"id":"build","name":"synthetic-build","version":"1.0.0","source":"registry+https://example.invalid"},
                {"id":"dev","name":"synthetic-test","version":"1.0.0","source":"registry+https://example.invalid"},
                {"id":"other-target","name":"synthetic-other-os","version":"1.0.0","source":"registry+https://example.invalid"}
            ],
            "resolve":{"nodes":[
                {"id":"root","deps":[
                    {"pkg":"runtime","dep_kinds":[{"kind":null}]},
                    {"pkg":"build","dep_kinds":[{"kind":"build"}]},
                    {"pkg":"dev","dep_kinds":[{"kind":"dev"}]}
                ]},
                {"id":"runtime","deps":[]},{"id":"build","deps":[]},
                {"id":"dev","deps":[]},{"id":"other-target","deps":[]}
            ]}
        });
        assert_eq!(
            macos_updater_dependencies(&metadata).unwrap(),
            BTreeSet::from([
                "synthetic-build-1.0.0".into(),
                "synthetic-runtime-1.0.0".into()
            ])
        );
        let mut incomplete = metadata;
        incomplete["resolve"]["nodes"]
            .as_array_mut()
            .unwrap()
            .remove(1);
        assert!(macos_updater_dependencies(&incomplete).is_err());
    }

    #[test]
    fn resolves_runtime_notices_from_a_minimal_toolchain_without_rust_docs() {
        let fixture = tempfile::tempdir().unwrap();
        for path in [
            "share/doc/rust/COPYRIGHT-library.html",
            "share/doc/cargo/LICENSE-MIT",
            "share/doc/cargo/LICENSE-APACHE",
        ] {
            let path = fixture.path().join(path);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, "synthetic notice").unwrap();
        }
        assert_eq!(rust_runtime_notices(fixture.path()).unwrap().len(), 3);
        fs::remove_file(fixture.path().join("share/doc/cargo/LICENSE-MIT")).unwrap();
        assert!(rust_runtime_notices(fixture.path()).is_err());
    }

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
    fn release_verification_refuses_unlisted_assets() {
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
        let files = verify_checksums(root.path()).unwrap();
        require_only_listed_assets(root.path(), &files).unwrap();
        fs::write(root.path().join("extra.dmg"), b"unlisted").unwrap();
        assert!(require_only_listed_assets(root.path(), &files).is_err());
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

    #[test]
    fn publication_requires_matching_license_and_disclosure_policy() {
        let source = tempfile::tempdir().unwrap();
        let package = tempfile::tempdir().unwrap();
        let license = package.path().join("LICENSE");
        fs::write(source.path().join("LICENSE"), b"synthetic license").unwrap();
        fs::write(&license, b"synthetic license").unwrap();
        let files = vec![license.clone()];
        assert!(verify_release_policy(source.path(), package.path(), &[]).is_err());
        assert!(verify_release_policy(source.path(), package.path(), &files).is_err());
        fs::write(source.path().join("SECURITY.md"), b"synthetic policy").unwrap();
        verify_release_policy(source.path(), package.path(), &files).unwrap();
        fs::write(&license, b"different license").unwrap();
        assert!(verify_release_policy(source.path(), package.path(), &files).is_err());
    }
}
