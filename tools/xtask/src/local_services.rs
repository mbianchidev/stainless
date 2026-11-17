use super::{Workspace, package, run};
use anyhow::{Context, Result, ensure};
use serde_json::Value;
use stainless_release::sha256;
use std::fs;
use std::path::{Component, Path, PathBuf};

const REPORT: &str = "stainless-native-runtimes.json";

fn field<'a>(value: &'a Value, name: &str) -> Result<&'a str> {
    value[name]
        .as_str()
        .with_context(|| format!("Missing native runtime provenance field: {name}"))
}

pub(super) fn build(workspace: &Workspace) -> Result<Value> {
    let cache = std::env::var_os("STAINLESS_SERVICES_CACHE_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| workspace.cache.join("local-services"));
    ensure!(
        cache.is_absolute()
            && !cache.components().any(|part| part == Component::ParentDir)
            && !cache.starts_with(&workspace.root),
        "STAINLESS_SERVICES_CACHE_DIR must be an absolute external build cache"
    );
    let header = workspace
        .root
        .join("tools/local_services/runtime/stainless_local_runtime.h");
    let worker_header = workspace
        .source()
        .join("chrome/services/stainless_local/public/stainless_local_runtime.h");
    ensure!(
        fs::read(&header)? == fs::read(worker_header)?,
        "Chromium and native libraries do not use the same fixed C ABI"
    );
    run(workspace
        .chromium_command("python-bin/python3")?
        .current_dir(&workspace.root)
        .env("PYTHONDONTWRITEBYTECODE", "1")
        .arg(workspace.root.join("tools/local_services.py"))
        .arg("--cache")
        .arg(&cache)
        .arg("--chromium-cache")
        .arg(&workspace.cache)
        .arg("--build-runtimes")
        .arg("--runtime-report")
        .arg(workspace.output().join(REPORT)))?;
    let metadata: Value = serde_json::from_slice(&fs::read(workspace.output().join(REPORT))?)?;
    ensure!(
        field(&metadata, "manifest_sha256")?
            == sha256(&workspace.root.join("chromium/services-inputs.json"))?
            && field(&metadata, "runtime_header_sha256")? == sha256(&header)?,
        "Native runtime provenance does not match the reviewed inputs and C ABI"
    );
    Ok(metadata)
}

fn stage_libraries(metadata: &Value, libraries: &Path) -> Result<()> {
    ensure!(
        libraries.is_dir() && !libraries.symlink_metadata()?.file_type().is_symlink(),
        "Expected the actual built framework's native library directory"
    );
    for engine in ["translation", "ocr", "speech", "vision"] {
        let entry = &metadata["runtimes"][engine];
        let source = Path::new(field(entry, "library")?);
        ensure!(
            source.is_file()
                && !source.symlink_metadata()?.file_type().is_symlink()
                && sha256(source)? == field(entry, "sha256")?,
            "Native {engine} library changed after its verified build"
        );
        let destination = libraries.join(format!("libstainless_{engine}.dylib"));
        ensure!(
            !destination.is_symlink(),
            "Refusing a linked native runtime destination"
        );
        fs::copy(source, &destination)?;
        ensure!(
            sha256(&destination)? == field(entry, "sha256")?,
            "Staged native {engine} bytes differ"
        );
    }
    let notice = &metadata["runtime_notices"];
    let source = Path::new(field(notice, "path")?);
    ensure!(
        source.is_file()
            && !source.symlink_metadata()?.file_type().is_symlink()
            && sha256(source)? == field(notice, "sha256")?,
        "Native runtime license inventory differs"
    );
    let destination = libraries.join("StainlessLocalServicesNotices.txt");
    ensure!(
        !destination.is_symlink(),
        "Refusing a linked notice destination"
    );
    fs::copy(source, &destination)?;
    Ok(())
}

pub(super) fn stage(workspace: &Workspace, metadata: &Value, app: &Path) -> Result<Value> {
    let version = workspace
        .manifest
        .chromium
        .version
        .as_ref()
        .context("Missing Chromium framework version")?;
    let libraries = app
        .join("Contents/Frameworks/Stainless Framework.framework/Versions")
        .join(version)
        .join("Libraries");
    let test_framework = workspace.output().join("Stainless Framework.framework");
    let test_libraries = test_framework
        .join("Versions")
        .join(version)
        .join("Libraries");
    stage_libraries(metadata, &libraries)?;
    stage_libraries(metadata, &test_libraries)?;
    package::verify_dependencies(workspace, &test_framework)?;
    package::sign_inside_out(&test_framework)?;
    package::verify_dependencies(workspace, app)?;
    package::sign_inside_out(app)?;
    run(workspace
        .command("/usr/bin/codesign")
        .args(["--verify", "--deep", "--strict"])
        .arg(app))?;
    run(workspace
        .command("/usr/bin/codesign")
        .args(["--verify", "--deep", "--strict"])
        .arg(&test_framework))?;
    let mut staged = metadata.clone();
    for engine in ["translation", "ocr", "speech", "vision"] {
        let path = libraries.join(format!("libstainless_{engine}.dylib"));
        staged["runtimes"][engine]["staged_sha256"] = sha256(&path)?.into();
        let path = test_libraries.join(format!("libstainless_{engine}.dylib"));
        staged["runtimes"][engine]["test_framework_sha256"] = sha256(&path)?.into();
    }
    fs::write(
        workspace.output().join(REPORT),
        serde_json::to_vec_pretty(&staged)?,
    )?;
    Ok(staged)
}

pub(super) fn provenance(workspace: &Workspace) -> Result<Value> {
    serde_json::from_slice(&fs::read(workspace.output().join(REPORT))?)
        .context("Missing verified native runtime build provenance")
}
