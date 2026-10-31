mod package;
mod smoke;

use anyhow::{Context, Result, bail, ensure};
use fs2::FileExt;
use serde::{Deserialize, Serialize};
use stainless_release::{Product, is_hex, macos_version, sha256};
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Source {
    repository: String,
    revision: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    version: Option<String>,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct RustToolchain {
    version: String,
    archive_url: String,
    sha256: String,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Platform {
    os: String,
    arch: String,
    minimum_macos: String,
    minimum_xcode: String,
    minimum_sdk: String,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Manifest {
    schema_version: u32,
    chromium: Source,
    depot_tools: Source,
    rust: RustToolchain,
    platform: Platform,
    product: Product,
    patches: Vec<PathBuf>,
}

struct Workspace {
    root: PathBuf,
    cache: PathBuf,
    manifest: Manifest,
}

impl Workspace {
    fn load() -> Result<Self> {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(Path::parent)
            .context("Cannot locate repository root")?
            .to_owned();
        let manifest: Manifest =
            serde_json::from_slice(&fs::read(root.join("chromium/manifest.json"))?)
                .context("Invalid Chromium manifest")?;
        ensure!(manifest.schema_version == 1, "Unsupported source manifest");
        ensure!(
            is_hex(&manifest.chromium.revision, 40),
            "Chromium must use a full revision"
        );
        ensure!(
            is_hex(&manifest.depot_tools.revision, 40),
            "depot_tools must use a full revision"
        );
        ensure!(
            manifest.chromium.repository == "https://chromium.googlesource.com/chromium/src.git",
            "Untrusted Chromium source"
        );
        ensure!(
            manifest.depot_tools.repository
                == "https://chromium.googlesource.com/chromium/tools/depot_tools.git",
            "Untrusted depot_tools source"
        );
        manifest.product.validate()?;
        for patch in &manifest.patches {
            ensure!(
                patch.starts_with("chromium/patches")
                    && patch
                        .components()
                        .all(|part| matches!(part, std::path::Component::Normal(_))),
                "Patch path escapes the repository"
            );
        }
        let cache = std::env::var_os("STAINLESS_CACHE_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                PathBuf::from(std::env::var_os("HOME").unwrap_or_default())
                    .join("Library/Caches/StainlessBuild")
            });
        ensure!(cache.is_absolute(), "STAINLESS_CACHE_DIR must be absolute");
        ensure!(
            !cache.starts_with(&root),
            "Keep Chromium history outside the Stainless repository"
        );
        ensure!(
            !cache
                .components()
                .any(|part| matches!(part, std::path::Component::ParentDir)),
            "The build cache path cannot contain parent-directory components"
        );
        ensure!(
            !cache.to_string_lossy().contains(char::is_whitespace),
            "Chromium's source/cache path cannot contain whitespace"
        );
        Ok(Self {
            root,
            cache,
            manifest,
        })
    }

    fn checkout(&self) -> PathBuf {
        self.cache
            .join("chromium")
            .join(&self.manifest.chromium.revision)
    }

    fn source(&self) -> PathBuf {
        self.checkout().join("src")
    }
    fn depot(&self) -> PathBuf {
        self.cache
            .join("depot_tools")
            .join(&self.manifest.depot_tools.revision)
    }
    fn output(&self) -> PathBuf {
        self.source().join("out/Stainless")
    }

    fn updater_binary(&self) -> PathBuf {
        self.root
            .join("target/aarch64-apple-darwin/release/stainless-update")
    }

    fn lock_cache(&self) -> Result<std::fs::File> {
        fs::create_dir_all(&self.cache)?;
        ensure!(
            !fs::canonicalize(&self.cache)?.starts_with(fs::canonicalize(&self.root)?),
            "Build cache resolves inside the Stainless checkout"
        );
        let lock = std::fs::File::options()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(
                self.cache
                    .join(format!(".source-{}.lock", self.manifest.chromium.revision)),
            )?;
        lock.try_lock_exclusive().context(
            "Another Stainless source/build/package operation uses this cache. Wait or choose another STAINLESS_CACHE_DIR"
        )?;
        Ok(lock)
    }

    fn chromium_command(&self, tool: &str) -> Result<Command> {
        let mut command = Command::new(self.depot().join(tool));
        let mut paths = vec![self.depot()];
        paths.extend(std::env::split_paths(
            &std::env::var_os("PATH").unwrap_or_default(),
        ));
        command
            .env("PATH", std::env::join_paths(paths)?)
            .env_remove("GOOGLE_API_KEY")
            .env_remove("GOOGLE_DEFAULT_CLIENT_ID")
            .env_remove("GOOGLE_DEFAULT_CLIENT_SECRET")
            .env("DEPOT_TOOLS_UPDATE", "0")
            .env("DEPOT_TOOLS_METRICS", "0")
            .env("DEPOT_TOOLS_WIN_TOOLCHAIN", "0");
        Ok(command)
    }

    fn doctor(&self) -> Result<()> {
        ensure!(
            std::env::consts::OS == "macos" && std::env::consts::ARCH == "aarch64",
            "Source builds currently require Apple Silicon macOS"
        );
        let xcode = output(Command::new("xcodebuild").arg("-version"))
            .context("Full Xcode is required. Install Xcode 26.6 or newer, accept its license, and select it with DEVELOPER_DIR or xcode-select. Command Line Tools alone are not supported")?;
        let version = xcode
            .lines()
            .next()
            .and_then(|line| line.strip_prefix("Xcode "))
            .context("Cannot determine Xcode version")?;
        ensure!(
            macos_version(version)? >= macos_version(&self.manifest.platform.minimum_xcode)?,
            "Xcode {} or newer is required; found {version}",
            self.manifest.platform.minimum_xcode
        );
        let sdk = output(Command::new("xcrun").arg("--show-sdk-version"))?;
        ensure!(
            macos_version(sdk.trim())? >= macos_version(&self.manifest.platform.minimum_sdk)?,
            "macOS SDK {} or newer is required; found {}",
            self.manifest.platform.minimum_sdk,
            sdk.trim()
        );
        let rust = output(Command::new("rustc").arg("--version"))?;
        ensure!(
            rust.split_whitespace().nth(1) == Some(self.manifest.rust.version.as_str()),
            "Use pinned Rust {}",
            self.manifest.rust.version
        );
        let memory: u64 = output(Command::new("sysctl").args(["-n", "hw.memsize"]))?
            .trim()
            .parse()?;
        ensure!(
            memory >= 16 * 1024 * 1024 * 1024,
            "At least 16 GiB RAM is required for a Chromium build"
        );
        fs::create_dir_all(&self.cache)?;
        let disk: serde_json::Value = serde_json::to_value(
            plist::Value::from_reader_xml(
                &run_output(
                    Command::new("diskutil")
                        .args(["info", "-plist"])
                        .arg(&self.cache),
                )?
                .stdout[..],
            )
            .context("Cannot inspect the build volume")?,
        )?;
        ensure!(
            disk["FilesystemType"] == "apfs",
            "Chromium requires an APFS build volume"
        );
        let available = fs2_free_space(&self.cache)?;
        ensure!(
            available >= 100 * 1024 * 1024 * 1024,
            "Need at least 100 GiB free for source/build output"
        );
        println!(
            "Apple Silicon macOS; {version}; SDK {}; Rust {}; {} GiB available",
            sdk.trim(),
            self.manifest.rust.version,
            available / 1024 / 1024 / 1024
        );
        Ok(())
    }

    fn fetch(&self, jobs: usize) -> Result<()> {
        fs::create_dir_all(&self.cache)?;
        pinned_git_checkout(&self.depot(), &self.manifest.depot_tools)?;
        fs::create_dir_all(self.checkout())?;
        let config = format!(
            "solutions = [{{'name': 'src', 'url': '{}', 'managed': False, 'deps_file': 'DEPS', 'custom_deps': {{}}, 'custom_vars': {{'checkout_pgo_profiles': False}}}}]\ntarget_os = ['mac']\n",
            self.manifest.chromium.repository
        );
        let config_path = self.checkout().join(".gclient");
        if config_path.exists() {
            ensure!(
                fs::read_to_string(&config_path)? == config,
                "Existing .gclient differs; refusing to overwrite"
            );
        } else {
            fs::write(&config_path, config)?;
        }
        if self.source().join(".git").exists() {
            self.check_source_revision()?;
            self.check_patch_state()?;
        }
        run(self
            .chromium_command("gclient")?
            .current_dir(self.checkout())
            .args(["sync", "--no-history", "--nohooks", "--revision"])
            .arg(format!("src@{}", self.manifest.chromium.revision))
            .arg(format!("--jobs={jobs}")))?;
        self.check_source_revision()?;
        self.apply_patches()
    }

    fn check_source_revision(&self) -> Result<()> {
        let revision = output(
            Command::new("git")
                .arg("-C")
                .arg(self.source())
                .args(["rev-parse", "HEAD"]),
        )?;
        ensure!(
            revision.trim() == self.manifest.chromium.revision,
            "Chromium checkout is on another revision"
        );
        Ok(())
    }

    fn patch_command(&self) -> Command {
        let mut command = Command::new("git");
        command.arg("-C").arg(self.source()).arg("apply");
        command
    }

    fn patch_paths(&self) -> Vec<PathBuf> {
        self.manifest
            .patches
            .iter()
            .map(|path| self.root.join(path))
            .collect()
    }

    fn check_patch_state(&self) -> Result<bool> {
        let patches = self.patch_paths();
        let applied = self
            .patch_command()
            .args(["--reverse", "--check"])
            .args(&patches)
            .output()?;
        if applied.status.success() {
            let temporary = tempfile::tempdir()?;
            let index = temporary.path().join("expected-index");
            run(Command::new("git")
                .arg("-C")
                .arg(self.source())
                .env("GIT_INDEX_FILE", &index)
                .args(["read-tree", "HEAD"]))?;
            run(self
                .patch_command()
                .env("GIT_INDEX_FILE", &index)
                .arg("--cached")
                .args(&patches))?;
            run(Command::new("git")
                .arg("-C")
                .arg(self.source())
                .env("GIT_INDEX_FILE", &index)
                .args(["diff", "--exit-code", "--quiet"]))
            .context(
                "Chromium has edits beyond the pinned patch stack; preserve them before syncing",
            )?;
            return Ok(true);
        }
        let clean = output(Command::new("git").arg("-C").arg(self.source()).args([
            "status",
            "--porcelain",
            "--untracked-files=no",
        ]))?;
        ensure!(
            clean.is_empty(),
            "Chromium has unrecognized edits; preserve them before syncing"
        );
        run(self.patch_command().arg("--check").args(patches))?;
        Ok(false)
    }

    fn apply_patches(&self) -> Result<()> {
        self.check_source_revision()?;
        if !self.check_patch_state()? {
            run(self.patch_command().args(self.patch_paths()))?;
        }
        println!("Pinned Chromium revision and complete patch stack verified");
        Ok(())
    }

    fn build(&self, jobs: usize) -> Result<()> {
        self.doctor()?;
        self.apply_patches()?;
        run(self
            .chromium_command("gclient")?
            .current_dir(self.checkout())
            .arg("runhooks"))?;
        fs::create_dir_all(self.output())?;
        fs::copy(
            self.root.join("chromium/args.gn"),
            self.output().join("args.gn"),
        )?;
        run(self
            .chromium_command("gn")?
            .current_dir(self.source())
            .args(["gen", "out/Stainless", "--fail-on-unused-args"]))?;
        run(self
            .chromium_command("autoninja")?
            .current_dir(self.source())
            .args(["-C", "out/Stainless", "-j"])
            .arg(jobs.to_string())
            .args(["chrome", "chromedriver"]))?;
        run(Command::new("cargo")
            .current_dir(&self.root)
            .env(
                "MACOSX_DEPLOYMENT_TARGET",
                &self.manifest.platform.minimum_macos,
            )
            .args([
                "build",
                "--locked",
                "--release",
                "--target",
                "aarch64-apple-darwin",
                "--package",
                "stainless-updater",
            ]))?;
        package::build_ui(self)?;
        self.write_build_manifest()?;
        Ok(())
    }

    fn write_build_manifest(&self) -> Result<()> {
        let patches: Result<Vec<_>> = self
            .manifest
            .patches
            .iter()
            .map(|path| {
                Ok(serde_json::json!({"path": path, "sha256": sha256(&self.root.join(path))?}))
            })
            .collect();
        let data = serde_json::json!({
            "schema_version": 1,
            "source": self.manifest,
            "product_revision": output(Command::new("git").current_dir(&self.root).args(["rev-parse", "HEAD"]))?.trim(),
            "dirty": !output(Command::new("git").current_dir(&self.root).args(["status", "--porcelain"]))?.is_empty(),
            "patches": patches?,
            "gn_args_sha256": sha256(&self.output().join("args.gn"))?,
            "xcode": output(Command::new("xcodebuild").arg("-version"))?.trim(),
            "sdk": output(Command::new("xcrun").arg("--show-sdk-version"))?.trim(),
            "rustc": output(Command::new("rustc").arg("--version"))?.trim(),
            "cargo_lock_sha256": sha256(&self.root.join("Cargo.lock"))?,
            "browser_executable_sha256": sha256(&self.output().join("Stainless.app/Contents/MacOS/Stainless"))?,
            "updater_executable_sha256": sha256(&self.updater_binary())?,
            "rust_target": "aarch64-apple-darwin",
            "macos_deployment_target": self.manifest.platform.minimum_macos,
            "updater_ui_sha256": sha256(&self.root.join("target/native/Stainless Updater"))?,
            "chromium_deps_sha256": sha256(&self.source().join("DEPS"))?,
            "resolved_dependencies": output(self.chromium_command("gclient")?
                .current_dir(self.checkout()).args(["revinfo", "--actual"]))?,
            "swift": output(Command::new("xcrun").args(["swiftc", "--version"]))?.trim(),
            "macos": output(Command::new("sw_vers").arg("-productVersion"))?.trim(),
        });
        fs::write(
            self.output().join("stainless-build.json"),
            serde_json::to_vec_pretty(&data)?,
        )?;
        Ok(())
    }

    fn security_status(&self) -> Result<()> {
        let response: Vec<serde_json::Value> = reqwest::blocking::Client::builder()
            .https_only(true)
            .timeout(std::time::Duration::from_secs(30))
            .build()?
            .get(
                "https://chromiumdash.appspot.com/fetch_releases?channel=Stable&platform=Mac&num=1",
            )
            .send()?
            .error_for_status()?
            .json()?;
        let latest = response
            .first()
            .context("Chromium security baseline unavailable")?;
        ensure!(
            latest["hashes"]["chromium"].as_str() == Some(&self.manifest.chromium.revision),
            "Pinned Chromium is superseded by {}. Rebase and review upstream security fixes before release",
            latest["version"]
        );
        println!(
            "Pinned Chromium matches the current Mac stable release: {}",
            latest["version"]
        );
        Ok(())
    }
}

fn pinned_git_checkout(path: &Path, source: &Source) -> Result<()> {
    if !path.join(".git").exists() {
        ensure!(
            !path.exists(),
            "Refusing to overwrite an existing non-Git tool directory"
        );
        fs::create_dir_all(path)?;
        run(Command::new("git").arg("init").arg("--quiet").arg(path))?;
        run(Command::new("git")
            .arg("-C")
            .arg(path)
            .args(["remote", "add", "origin"])
            .arg(&source.repository))?;
        run(Command::new("git")
            .arg("-C")
            .arg(path)
            .args(["fetch", "--quiet", "--depth=1", "origin"])
            .arg(&source.revision))?;
        run(Command::new("git").arg("-C").arg(path).args([
            "checkout",
            "--quiet",
            "--detach",
            "FETCH_HEAD",
        ]))?;
    }
    let revision = output(
        Command::new("git")
            .arg("-C")
            .arg(path)
            .args(["rev-parse", "HEAD"]),
    )?;
    ensure!(
        revision.trim() == source.revision,
        "Tool checkout revision mismatch: {}",
        path.display()
    );
    ensure!(
        output(Command::new("git").arg("-C").arg(path).args([
            "status",
            "--porcelain",
            "--untracked-files=no"
        ]))?
        .is_empty(),
        "Pinned tool checkout has local edits: {}",
        path.display()
    );
    Ok(())
}

fn fs2_free_space(path: &Path) -> Result<u64> {
    let text = output(Command::new("df").arg("-k").arg(path))?;
    let available = text
        .lines()
        .nth(1)
        .and_then(|line| line.split_whitespace().nth(3))
        .context("Cannot inspect free disk space")?
        .parse::<u64>()?;
    available
        .checked_mul(1024)
        .context("Disk capacity overflow")
}

fn run(command: &mut Command) -> Result<()> {
    eprintln!("Running: {command:?}");
    let status = command
        .status()
        .with_context(|| format!("Cannot start {command:?}"))?;
    ensure!(status.success(), "Command failed ({status}): {command:?}");
    Ok(())
}

fn run_output(command: &mut Command) -> Result<Output> {
    let output = command
        .output()
        .with_context(|| format!("Cannot start {command:?}"))?;
    ensure!(
        output.status.success(),
        "Command failed: {command:?}\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    Ok(output)
}

fn output(command: &mut Command) -> Result<String> {
    String::from_utf8(run_output(command)?.stdout).context("Command output is not UTF-8")
}

fn parse_jobs(args: &mut Vec<OsString>) -> Result<usize> {
    if let Some(position) = args.iter().position(|arg| arg == "--jobs") {
        ensure!(position + 1 < args.len(), "--jobs needs a positive integer");
        let jobs = args
            .remove(position + 1)
            .to_string_lossy()
            .parse::<usize>()?;
        args.remove(position);
        ensure!((1..=64).contains(&jobs), "--jobs must be between 1 and 64");
        Ok(jobs)
    } else {
        Ok(8)
    }
}

fn main() {
    if let Err(error) = execute() {
        eprintln!("Stainless: {error:#}");
        std::process::exit(1);
    }
}

fn execute() -> Result<()> {
    let mut args: Vec<OsString> = std::env::args_os().skip(1).collect();
    let jobs = parse_jobs(&mut args)?;
    let command = args
        .first()
        .and_then(|arg| arg.to_str())
        .unwrap_or("help")
        .to_owned();
    if command == "help" || command == "--help" {
        println!(
            "Usage: ./tools/stainless COMMAND [--jobs N]\n\
            doctor          Check the macOS build prerequisites\n\
            bootstrap       Verify the host and fetch pinned Chromium/depot_tools\n\
            fetch           Fetch/apply source only (does not require full Xcode)\n\
            patch-check     Verify the pinned checkout and patch stack\n\
            build           Compile full chrome, chromedriver, and the updater\n\
            test            Run Rust tests and compile the native updater UI\n\
            package         Bundle the app, DMG, update ZIP/feed, notices/manifests\n\
            smoke PATH      Exercise a full browser with synthetic local fixtures\n\
            publish PATH    Publish a gated private prerelease from a package directory\n\
            security-status Check the pin against current Chromium stable"
        );
        return Ok(());
    }
    let workspace = Workspace::load()?;
    if !matches!(command.as_str(), "publish" | "smoke") {
        ensure!(
            args.len() == 1,
            "Unexpected arguments; run ./tools/stainless help"
        );
    }
    let _source_lock = if matches!(
        command.as_str(),
        "bootstrap" | "fetch" | "patch-check" | "build" | "package" | "smoke"
    ) {
        Some(workspace.lock_cache()?)
    } else {
        None
    };
    match command.as_str() {
        "doctor" => workspace.doctor(),
        "bootstrap" => {
            workspace.doctor()?;
            workspace.fetch(jobs)
        }
        "fetch" => workspace.fetch(jobs),
        "patch-check" => workspace.apply_patches(),
        "build" => workspace.build(jobs),
        "test" => {
            ensure!(args.len() == 1, "Unexpected test arguments");
            run(Command::new("cargo").current_dir(&workspace.root).args([
                "test",
                "--locked",
                "--workspace",
            ]))?;
            package::build_ui(&workspace)
        }
        "package" => package::package(&workspace),
        "smoke" => {
            ensure!(
                args.len() == 2,
                "smoke requires an exact Stainless.app path"
            );
            let report = workspace.root.join("target/browser-smoke.json");
            smoke::check(&workspace, Path::new(&args[1]), &report)
        }
        "publish" => {
            ensure!(
                args.len() == 2,
                "publish requires an exact package directory"
            );
            package::publish(&workspace, Path::new(&args[1]))
        }
        "security-status" => workspace.security_status(),
        _ => bail!("Unknown command {command}; run ./tools/stainless help"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_checked_in_source_manifest() {
        let workspace = Workspace::load().unwrap();
        assert_eq!(
            workspace.manifest.chromium.version.as_deref(),
            Some("156.0.8078.80")
        );
    }

    #[test]
    fn bounds_compile_concurrency() {
        assert_eq!(parse_jobs(&mut vec!["build".into()]).unwrap(), 8);
        assert_eq!(
            parse_jobs(&mut vec!["build".into(), "--jobs".into(), "4".into()]).unwrap(),
            4
        );
        assert!(parse_jobs(&mut vec!["--jobs".into(), "0".into()]).is_err());
        assert!(parse_jobs(&mut vec!["--jobs".into()]).is_err());
    }

    #[test]
    fn rejects_extra_edits_even_when_the_expected_patch_can_be_reversed() {
        let fixture = tempfile::tempdir().unwrap();
        let mut workspace = Workspace::load().unwrap();
        workspace.root = fixture.path().join("product");
        workspace.cache = fixture.path().join("cache");
        workspace.manifest.patches = vec![PathBuf::from("chromium/patches/fixture.patch")];
        fs::create_dir_all(workspace.source()).unwrap();
        fs::create_dir_all(workspace.root.join("chromium/patches")).unwrap();
        run(Command::new("git")
            .arg("-C")
            .arg(workspace.source())
            .args(["init", "--quiet"]))
        .unwrap();
        fs::write(
            workspace.source().join("fixture.txt"),
            "synthetic baseline\none\ntwo\nthree\nfour\nfive\nsix\nseven\n",
        )
        .unwrap();
        run(Command::new("git")
            .arg("-C")
            .arg(workspace.source())
            .args(["add", "fixture.txt"]))
        .unwrap();
        run(Command::new("git").arg("-C").arg(workspace.source()).args([
            "-c",
            "user.name=Synthetic Fixture",
            "-c",
            "user.email=fixture@example.invalid",
            "commit",
            "--quiet",
            "--no-gpg-sign",
            "-m",
            "Record synthetic baseline",
        ]))
        .unwrap();
        fs::write(
            workspace.source().join("fixture.txt"),
            "synthetic patched value\none\ntwo\nthree\nfour\nfive\nsix\nseven\n",
        )
        .unwrap();
        let patch = output(
            Command::new("git")
                .arg("-C")
                .arg(workspace.source())
                .args(["diff", "--binary"]),
        )
        .unwrap();
        fs::write(workspace.root.join("chromium/patches/fixture.patch"), patch).unwrap();
        assert!(workspace.check_patch_state().unwrap());
        fs::write(
            workspace.source().join("fixture.txt"),
            "synthetic patched value\none\ntwo\nthree\nfour\nfive\nsix\nseven\n\nunrelated synthetic edit\n",
        )
        .unwrap();
        assert!(
            workspace
                .patch_command()
                .args(["--reverse", "--check"])
                .args(workspace.patch_paths())
                .status()
                .unwrap()
                .success()
        );
        assert!(workspace.check_patch_state().is_err());
    }
}
