use anyhow::{Context, Result, bail};
use serde::Serialize;
use stainless_profiles::protocol::{MAX_REQUEST_BYTES, MetadataRequest};
use stainless_profiles::{BackupId, ProfileId, Store};
use std::ffi::OsString;
use std::io::{Read, Write};
use std::path::PathBuf;

const HELP: &str = "\
Usage: ./tools/stainless profiles COMMAND
  list                          List registered metadata, not browsing data
  create LABEL                  Create an empty, separately owned profile root
  metadata                      Read one versioned metadata-only JSON request on stdin
  run APP PROFILE_ID            Own a foreground browser process until it exits
  import-preview APP LABEL      Back up/copy the default preview; leave it untouched
  backup PROFILE_ID             Snapshot a closed managed profile
  restore BACKUP_ID LABEL       Verify/copy a backup into a NEW profile

APP is an explicit Stainless.app path. Close every Stainless browser and finish
pending updates before import, backup or restore. This developer tool is not
installed-profile onboarding, a sandboxed IPC service, or whole-profile encryption.
See docs/profile-isolation.md.";

enum Operation {
    List,
    Create(String),
    Metadata,
    Run(PathBuf, ProfileId),
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
                (Some("list"), 1) => Self::List,
                (Some("create"), 2) => Self::Create(text(1)?.to_owned()),
                (Some("metadata"), 1) => Self::Metadata,
                (Some("run"), 3) => Self::Run(
                    PathBuf::from(&args[1]),
                    ProfileId::try_from(text(2)?.to_owned())?,
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
        Operation::List => report(&store.list()?),
        Operation::Create(label) => report(&store.create(&label)?),
        Operation::Metadata => {
            report(&store.metadata(request.context("Missing metadata request")?)?)
        }
        Operation::Run(app, id) => store.run(&id, &app),
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
            Operation::Run(_, _)
        ));
    }
}
