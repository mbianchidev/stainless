use anyhow::{Context, Result, bail};
use serde::Serialize;
use stainless_profiles::modes::{LaunchRequest, Mode};
use stainless_profiles::protocol::{MAX_REQUEST_BYTES, MetadataRequest};
use stainless_profiles::{BackupId, ProfileId, Store};
use std::ffi::OsString;
use std::io::{Read, Write};
use std::path::PathBuf;

const HELP: &str = "\
Usage: ./tools/stainless profiles COMMAND
  modes                         Report enabled policies and unavailable modes
  list                          List registered metadata, not browsing data
  create LABEL                  Create an empty, separately owned profile root
  metadata                      Read one versioned metadata-only JSON request on stdin
  run APP PROFILE_ID [MODE]     Own a foreground Standard (default) or Incognito session
  safe-mode APP PROFILE_ID      Temporarily suppress risky Standard startup; retain saved data
  open APP PROFILE_ID MODE URL  Explicitly open ONLY an HTTP(S) URL in a new session
  import-preview APP LABEL      Back up/copy the default preview; leave it untouched
  backup PROFILE_ID             Snapshot a closed managed profile
  restore BACKUP_ID LABEL       Verify/copy a backup into a NEW profile

APP is an explicit Stainless.app path. Close every Stainless browser and finish
pending updates before import, backup or restore. This developer tool is not
installed-profile onboarding, a sandboxed IPC service, or whole-profile encryption.
Managed Incognito uses a fresh temporary root, not the profile's cookies/passwords.
Safe Mode keeps the Standard identity. Quit it, then explicitly run normally to return.
Downloads remain on disk. A killed supervisor can leave temporary session files.
Opening a URL does not move a live tab or copy storage, credentials or other artifacts.
See docs/profile-isolation.md.";

enum Operation {
    Modes,
    List,
    Create(String),
    Metadata,
    Run(PathBuf, ProfileId, LaunchRequest),
    Import(PathBuf, String),
    Backup(ProfileId),
    Restore(BackupId, String),
}

impl Operation {
    fn parse(args: &[OsString]) -> Result<Self> {
        let text = |index: usize| {
            args[index]
                .to_str()
                .context("Profile arguments must be valid UTF-8")
        };
        Ok(
            match (args.first().and_then(|value| value.to_str()), args.len()) {
                (Some("modes"), 1) => Self::Modes,
                (Some("list"), 1) => Self::List,
                (Some("create"), 2) => Self::Create(text(1)?.to_owned()),
                (Some("metadata"), 1) => Self::Metadata,
                (Some("run"), 3 | 4) => Self::Run(
                    PathBuf::from(&args[1]),
                    ProfileId::try_from(text(2)?.to_owned())?,
                    LaunchRequest::new(
                        if args.len() == 4 {
                            text(3)?.parse()?
                        } else {
                            Mode::Standard
                        },
                        None,
                    )?,
                ),
                (Some("safe-mode"), 3) => Self::Run(
                    PathBuf::from(&args[1]),
                    ProfileId::try_from(text(2)?.to_owned())?,
                    LaunchRequest::safe_mode(),
                ),
                (Some("open"), 5) => Self::Run(
                    PathBuf::from(&args[1]),
                    ProfileId::try_from(text(2)?.to_owned())?,
                    LaunchRequest::new(text(3)?.parse()?, Some(text(4)?))?,
                ),
                (Some("import-preview"), 3) => {
                    Self::Import(PathBuf::from(&args[1]), text(2)?.to_owned())
                }
                (Some("backup"), 2) => Self::Backup(ProfileId::try_from(text(1)?.to_owned())?),
                (Some("restore"), 3) => Self::Restore(
                    BackupId::try_from(text(1)?.to_owned())?,
                    text(2)?.to_owned(),
                ),
                _ => bail!("Unknown profile command or unexpected arguments.\n{HELP}"),
            },
        )
    }
}

pub(super) fn execute(args: &[OsString]) -> Result<()> {
    if args.is_empty() || (args.len() == 1 && matches!(args[0].to_str(), Some("help" | "--help"))) {
        println!("{HELP}");
        return Ok(());
    }
    let operation = Operation::parse(args)?;
    if matches!(operation, Operation::Modes) {
        return report(&Mode::ALL.map(Mode::policy));
    }
    let request = if matches!(operation, Operation::Metadata) {
        let mut bytes = Vec::new();
        std::io::stdin()
            .take(MAX_REQUEST_BYTES as u64 + 1)
            .read_to_end(&mut bytes)?;
        Some(MetadataRequest::parse(&bytes)?)
    } else {
        None
    };
    let home = PathBuf::from(std::env::var_os("HOME").context("HOME is unset")?);
    let store = Store::for_home(&home)?;
    match operation {
        Operation::Modes => unreachable!("Mode policies do not access profile storage"),
        Operation::List => report(&store.list()?),
        Operation::Create(label) => report(&store.create(&label)?),
        Operation::Metadata => {
            report(&store.metadata(request.context("Missing metadata request")?)?)
        }
        Operation::Run(app, id, request) => store.launch(&id, &app, &request),
        Operation::Import(app, label) => report(&store.import_preview(&label, &app)?),
        Operation::Backup(id) => report(&store.backup(&id)?),
        Operation::Restore(id, label) => report(&store.restore(&id, &label)?),
    }
}

fn report(value: &impl Serialize) -> Result<()> {
    let mut output = std::io::stdout().lock();
    serde_json::to_writer_pretty(&mut output, value)?;
    output.write_all(b"\n")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_data_paths_and_extra_browser_switches_instead_of_forwarding_them() {
        for arguments in [
            vec![
                "run",
                "/Applications/Stainless.app",
                "p-1234567890123456",
                "--no-sandbox",
            ],
            vec!["run", "/Applications/Stainless.app", "../another-profile"],
            vec!["restore", "/tmp/backup", "Synthetic"],
            vec![
                "import-preview",
                "/Applications/Stainless.app",
                "Synthetic",
                "/tmp/profile",
            ],
        ] {
            let arguments: Vec<_> = arguments.into_iter().map(OsString::from).collect();
            assert!(Operation::parse(&arguments).is_err());
        }
        let arguments =
            ["run", "/Applications/Stainless.app", "p-1234567890123456"].map(OsString::from);
        assert!(matches!(
            Operation::parse(&arguments).unwrap(),
            Operation::Run(_, _, _)
        ));
    }
}
