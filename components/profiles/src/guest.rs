//! Memory-only Guest sessions: a fresh browser root on a private, Finder-hidden
//! RAM disk that is ejected when the browser exits.

use crate::modes::{LaunchRequest, Mode};
use crate::process::{Child, PrivateDirectories, run_child};
use crate::{Store, app, files};
use anyhow::{Context, Result, bail, ensure};
use fs2::FileExt;
use std::ffi::{CStr, CString};
use std::fs;
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

pub const VOLUME_BYTES: u64 = 1 << 30;
const VOLUME_NAME: &str = "Stainless Guest";
const DISKUTIL: &str = "/usr/sbin/diskutil";
const HDIUTIL: &str = "/usr/bin/hdiutil";
const NEWFS_HFS: &str = "/sbin/newfs_hfs";

/// A RAM-backed HFS+ volume mounted with `nobrowse` at a private directory.
/// Ejecting it frees the memory, so its contents never reach persistent storage.
pub(crate) struct RamVolume {
    device: String,
    mount: PathBuf,
}

impl RamVolume {
    pub(crate) fn create(mount: &Path, bytes: u64) -> Result<Self> {
        ensure!(
            bytes >= 64 << 20 && bytes.is_multiple_of(512),
            "Invalid Guest volume size"
        );
        let mount = fs::canonicalize(mount).context("Cannot locate the Guest mount point")?;
        ensure!(
            fs::read_dir(&mount)?.next().is_none(),
            "The Guest mount point is not empty"
        );
        let device = attach(bytes / 512)?;
        let volume = Self { device, mount };
        if let Err(error) = volume.format_and_mount() {
            if let Err(eject) = volume.eject() {
                eprintln!("Cannot eject the unused Guest RAM disk: {eject:#}");
            }
            return Err(error);
        }
        Ok(volume)
    }

    fn format_and_mount(&self) -> Result<()> {
        run(Command::new(NEWFS_HFS)
            .args(["-v", VOLUME_NAME])
            .arg(&self.device))?;
        run(Command::new(DISKUTIL)
            .args(["mount", "-mountOptions", "nobrowse", "-mountPoint"])
            .arg(&self.mount)
            .arg(&self.device))?;
        self.verify()?;
        // Keep Spotlight from indexing private browsing pages.
        fs::write(self.mount.join(".metadata_never_index"), b"")?;
        Ok(())
    }

    pub(crate) fn path(&self) -> &Path {
        &self.mount
    }

    pub(crate) fn verify(&self) -> Result<()> {
        let mounted = mounted(&self.mount)?.context("The Guest RAM disk is not mounted")?;
        ensure!(
            mounted.device == self.device && mounted.hidden && mounted.hfs,
            "The Guest mount point is not this session's hidden RAM disk"
        );
        Ok(())
    }

    pub(crate) fn eject(&self) -> Result<()> {
        eject(&self.device, &self.mount)
    }
}

struct Mounted {
    device: String,
    hidden: bool,
    hfs: bool,
}

fn attach(sectors: u64) -> Result<String> {
    let image = format!("ram://{sectors}");
    // diskutil image replaces the deprecated hdiutil verb on newer macOS releases.
    let output = Command::new(DISKUTIL)
        .args(["image", "attach", "--noMount", &image])
        .stdin(Stdio::null())
        .output()
        .ok()
        .filter(|output| output.status.success())
        .map_or_else(
            || {
                Command::new(HDIUTIL)
                    .args(["attach", "-nomount", &image])
                    .stdin(Stdio::null())
                    .output()
                    .context("Cannot create the Guest RAM disk")
            },
            Ok,
        )?;
    ensure!(
        output.status.success(),
        "Cannot create the Guest RAM disk: {}",
        String::from_utf8_lossy(&output.stderr).trim()
    );
    device(&String::from_utf8_lossy(&output.stdout))
}

fn device(output: &str) -> Result<String> {
    output
        .split_whitespace()
        .find(|word| {
            word.strip_prefix("/dev/disk").is_some_and(|number| {
                !number.is_empty() && number.bytes().all(|byte| byte.is_ascii_digit())
            })
        })
        .map(str::to_owned)
        .context("The RAM disk tool did not report a whole-disk device")
}

fn eject(device: &str, mount: &Path) -> Result<()> {
    ensure!(
        device.strip_prefix("/dev/disk").is_some_and(|number| {
            !number.is_empty() && number.bytes().all(|byte| byte.is_ascii_digit())
        }),
        "Refusing to eject an unexpected device"
    );
    if run(Command::new(DISKUTIL).args(["eject", device])).is_err() {
        // The browser has exited; release any lingering handles on its private volume.
        run(Command::new(DISKUTIL).args(["unmountDisk", "force", device]))?;
        run(Command::new(DISKUTIL).args(["eject", device]))?;
    }
    ensure!(
        mounted(mount)?.is_none(),
        "The Guest RAM disk is still mounted at {}",
        mount.display()
    );
    Ok(())
}

fn mounted(path: &Path) -> Result<Option<Mounted>> {
    let path = fs::canonicalize(path)?;
    let text = CString::new(path.as_os_str().as_bytes())?;
    let mut status: libc::statfs = unsafe { std::mem::zeroed() };
    ensure!(
        unsafe { libc::statfs(text.as_ptr(), &mut status) } == 0,
        "Cannot inspect {}: {}",
        path.display(),
        std::io::Error::last_os_error()
    );
    let field = |value: &[libc::c_char]| {
        unsafe { CStr::from_ptr(value.as_ptr()) }
            .to_string_lossy()
            .into_owned()
    };
    if Path::new(&field(&status.f_mntonname)) != path {
        return Ok(None);
    }
    Ok(Some(Mounted {
        device: field(&status.f_mntfromname),
        hidden: status.f_flags & libc::MNT_DONTBROWSE as u32 != 0,
        hfs: field(&status.f_fstypename) == "hfs",
    }))
}

fn run(command: &mut Command) -> Result<()> {
    let output = command
        .stdin(Stdio::null())
        .output()
        .with_context(|| format!("Cannot start {command:?}"))?;
    ensure!(
        output.status.success(),
        "{command:?} failed: {}",
        String::from_utf8_lossy(&output.stderr).trim()
    );
    Ok(())
}

impl Store {
    pub fn launch_guest(&self, app_path: &Path, request: &LaunchRequest) -> Result<()> {
        self.launch_guest_owned(app_path, request, || Ok(()), Stdio::inherit())
    }

    pub fn launch_guest_observed(
        &self,
        app_path: &Path,
        request: &LaunchRequest,
        started: impl FnOnce() -> Result<()>,
    ) -> Result<()> {
        self.launch_guest_owned(app_path, request, started, Stdio::null())
    }

    fn launch_guest_owned(
        &self,
        app_path: &Path,
        request: &LaunchRequest,
        started: impl FnOnce() -> Result<()>,
        stdout: Stdio,
    ) -> Result<()> {
        ensure!(request.mode() == Mode::Guest, "Not a Guest launch request");
        request.mode().require_available()?;
        let _installation = self.installation_lock(false)?;
        self.check_update_transaction()?;
        let (app, _browser) = app::identity(app_path)?;
        app::require_managed_modes(&app)?;
        app::require_managed_guest(&app)?;
        let sessions = self.root.join("guest");
        files::directory(&sessions, true)?;
        // Held while sessions are created or cleaned, so a session without its owner
        // lock is always an orphan, never one another launcher is still creating.
        let creation = files::open_lock(&sessions.join("sessions.lock"))?;
        creation
            .lock_exclusive()
            .context("Cannot lock Guest session creation")?;
        for warning in clean_stale_sessions(&sessions)? {
            eprintln!("{warning}");
        }
        let session = tempfile::Builder::new()
            .prefix("g-")
            .tempdir_in(&sessions)?
            .keep();
        let owner = files::open_lock(&session.join("owner.lock"))?;
        owner
            .try_lock_exclusive()
            .context("Cannot own the new Guest session")?;
        drop(creation);
        let mount = session.join("volume");
        files::directory(&mount, true)?;
        let volume = match RamVolume::create(&mount, VOLUME_BYTES) {
            Ok(volume) => volume,
            Err(error) => {
                remove_session(&session)?;
                return Err(error);
            }
        };
        let mut quiescent = true;
        let result = (|| {
            let root = volume.path();
            for name in ["data", "cache", "tmp", "crashpad"] {
                files::directory(&root.join(name), true)?;
            }
            run_child(
                Child {
                    app: &app,
                    data: &root.join("data"),
                    cache: &root.join("cache"),
                    private: Some(PrivateDirectories {
                        temporary: &root.join("tmp"),
                        crash_dumps: &root.join("crashpad"),
                    }),
                },
                request,
                &mut quiescent,
                started,
                stdout,
            )
        })();
        let data = volume.path().join("data");
        if quiescent && let Some(pid) = live_singleton(&data) {
            // Only a browser that outlived the launched process, such as a relaunch, can
            // hold the Guest root now.
            if let Err(browser_error) = &result {
                eprintln!("Guest browser failed: {browser_error:#}");
            }
            bail!(
                "Guest browser process {pid} still uses the RAM disk at {}; quit it, then open Guest again to eject it",
                mount.display()
            );
        }
        if !quiescent {
            return result.with_context(|| {
                format!(
                    "Browser ownership is uncertain; the Guest RAM disk stays mounted at {}. Its contents remain in memory only. Close the browser; the next Guest launch ejects it",
                    mount.display()
                )
            });
        }
        if let Err(error) = volume.eject() {
            if let Err(browser_error) = &result {
                eprintln!("Guest browser failed: {browser_error:#}");
            }
            return Err(error).with_context(|| {
                format!(
                    "Cannot eject the Guest RAM disk at {}. Its contents remain in memory only and disappear on restart; the next Guest launch retries",
                    mount.display()
                )
            });
        }
        drop(owner);
        remove_session(&session)?;
        result
    }
}

/// The live process named by Chromium's `SingletonLock` ("hostname-pid") in `data`.
fn live_singleton(data: &Path) -> Option<i32> {
    let target = fs::read_link(data.join("SingletonLock")).ok()?;
    let pid: i32 = target.to_str()?.rsplit('-').next()?.parse().ok()?;
    if pid <= 0 {
        return None;
    }
    let alive = unsafe { libc::kill(pid, 0) } == 0
        || std::io::Error::last_os_error().raw_os_error() == Some(libc::EPERM);
    alive.then_some(pid)
}

fn remove_session(session: &Path) -> Result<()> {
    let mount = session.join("volume");
    if mount.exists() {
        ensure!(
            mounted(&mount)?.is_none(),
            "Refusing to remove a mounted Guest session"
        );
        fs::remove_dir(&mount)?;
    }
    match fs::remove_file(session.join("owner.lock")) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.into()),
    }
    fs::remove_dir(session).with_context(|| format!("Cannot remove {}", session.display()))
}

/// Ejects RAM disks left by a supervisor that exited without teardown. A live
/// owner, or a browser that may still be running, keeps its session.
fn clean_stale_sessions(sessions: &Path) -> Result<Vec<String>> {
    let mut warnings = Vec::new();
    for entry in fs::read_dir(sessions)? {
        let entry = entry?;
        let session = entry.path();
        let name = entry.file_name();
        if !name.to_string_lossy().starts_with("g-") {
            continue;
        }
        files::existing_directory(&session)?;
        let owned = fs::symlink_metadata(session.join("owner.lock")).is_ok();
        let owner = if owned {
            let owner = files::open_lock(&session.join("owner.lock"))?;
            if owner.try_lock_exclusive().is_err() {
                continue;
            }
            Some(owner)
        } else {
            None
        };
        let mount = session.join("volume");
        let cleanup = (|| {
            if let Some(pid) = live_singleton(&mount.join("data")) {
                bail!(
                    "Guest browser process {pid} may still use {}; quit it, then open Guest again to clean it up",
                    mount.display()
                );
            }
            match mounted(&mount) {
                Ok(Some(volume)) => eject(&volume.device, &mount)?,
                Ok(None) => {}
                Err(error) if !mount.exists() => drop(error),
                Err(error) => return Err(error),
            }
            // Files outside a RAM disk, for example a lockless orphan's root that a
            // browser recreated on disk, were meant to be memory-only: delete them.
            if mount.exists() {
                fs::remove_dir_all(&mount)?;
            }
            remove_session(&session)
        })();
        drop(owner);
        if let Err(error) = cleanup {
            warnings.push(format!("Retained a previous Guest session: {error:#}"));
        }
    }
    Ok(warnings)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_only_whole_disk_devices() {
        assert_eq!(device("/dev/disk12\n").unwrap(), "/dev/disk12");
        assert_eq!(device("/dev/disk12          \t\n").unwrap(), "/dev/disk12");
        for output in ["", "/dev/disk12s1", "/dev/diskX", "disk12", "/dev/rdisk3"] {
            assert!(device(output).is_err(), "{output}");
        }
        assert!(eject("/dev/disk1s1", Path::new("/")).is_err());
    }

    #[test]
    fn keeps_guest_data_on_a_hidden_ram_disk_until_ejected() {
        let fixture = tempfile::tempdir().unwrap();
        let mount = fixture.path().join("volume");
        fs::create_dir(&mount).unwrap();
        let volume = RamVolume::create(&mount, 64 << 20).unwrap();
        volume.verify().unwrap();
        let file = volume.path().join("synthetic-guest-state");
        fs::write(&file, b"synthetic memory-only state").unwrap();
        assert!(
            RamVolume::create(&mount, 64 << 20).is_err(),
            "a second volume cannot reuse a mounted session"
        );
        volume.eject().unwrap();
        assert!(mounted(&mount).unwrap().is_none());
        assert!(!file.exists());
        assert!(fs::read_dir(&mount).unwrap().next().is_none());
    }
}
