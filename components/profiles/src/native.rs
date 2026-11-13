use crate::modes::{LaunchRequest, Mode};
use crate::protocol::{MAX_REQUEST_BYTES, MetadataRequest, MetadataResponse};
use crate::{BackupCatalog, BackupId, ProfileId, REGISTRY_VERSION, Store, app};
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use std::fs;
use std::io::{Read, Write};
use std::path::Path;

pub const PROTOCOL_VERSION: u32 = 1;
pub const RECOVERY_VERSION: u32 = 1;
pub const SAFE_MODE_VERSION: u32 = 1;
pub const HELPER_PATH: &str =
    "Contents/Helpers/Stainless Updater.app/Contents/Resources/stainless-update";

#[derive(Clone, Copy)]
pub enum Operation {
    Metadata,
    Launch,
    Recovery,
}

impl Operation {
    pub fn from_command(command: &str) -> Option<Self> {
        match command {
            "profile-metadata" => Some(Self::Metadata),
            "profile-launch" => Some(Self::Launch),
            "profile-recovery" => Some(Self::Recovery),
            _ => None,
        }
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct NativeLaunch {
    protocol_version: u32,
    profile_id: ProfileId,
    mode: Mode,
    #[serde(default)]
    safe_mode: bool,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct NativeRecovery {
    protocol_version: u32,
    request: RecoveryOperation,
}

#[derive(Deserialize)]
#[serde(tag = "operation", rename_all = "snake_case", deny_unknown_fields)]
enum RecoveryOperation {
    List {
        after: Option<BackupId>,
    },
    Backup {
        profile_id: ProfileId,
        confirmed: bool,
    },
    Restore {
        backup_id: BackupId,
        label: String,
        confirmed: bool,
    },
}

#[derive(Serialize)]
struct Reply<'a> {
    protocol_version: u32,
    state: &'a str,
    message: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    metadata: Option<MetadataResponse>,
    #[serde(skip_serializing_if = "Option::is_none")]
    catalog: Option<BackupCatalog>,
    #[serde(skip_serializing_if = "Option::is_none")]
    backup_id: Option<BackupId>,
    #[serde(skip_serializing_if = "Option::is_none")]
    safe_mode: Option<bool>,
}

impl<'a> Reply<'a> {
    fn new(state: &'a str, message: &'a str) -> Self {
        Self {
            protocol_version: PROTOCOL_VERSION,
            state,
            message,
            metadata: None,
            catalog: None,
            backup_id: None,
            safe_mode: None,
        }
    }
}

fn report(output: &mut impl Write, state: &str, message: &str) -> Result<()> {
    write_reply(output, Reply::new(state, message))
}

fn write_reply(output: &mut impl Write, reply: Reply<'_>) -> Result<()> {
    serde_json::to_writer(&mut *output, &reply)?;
    output.write_all(b"\n")?;
    output
        .flush()
        .context("Cannot report native profile result")
}

pub fn report_error(output: &mut impl Write, error: &anyhow::Error) -> Result<()> {
    report(output, "error", &format!("{error:#}"))
}

/// One local native invocation, never an ambient renderer or metadata launch capability.
pub fn execute(
    operation: Operation,
    app_path: &Path,
    executable: &Path,
    home: &Path,
    input: impl Read,
    output: &mut impl Write,
) -> Result<()> {
    let mut bytes = Vec::new();
    input
        .take(MAX_REQUEST_BYTES as u64 + 1)
        .read_to_end(&mut bytes)?;
    ensure!(
        bytes.len() <= MAX_REQUEST_BYTES,
        "Native profile request is too large"
    );
    let metadata = match operation {
        Operation::Metadata => Some(MetadataRequest::parse(&bytes)?),
        _ => None,
    };
    let launch = match operation {
        Operation::Launch => {
            let launch: NativeLaunch =
                serde_json::from_slice(&bytes).context("Invalid native profile launch request")?;
            ensure!(
                launch.protocol_version == PROTOCOL_VERSION,
                "Unsupported native profile launch protocol"
            );
            ensure!(
                matches!(launch.mode, Mode::Standard | Mode::Incognito),
                "The native chooser supports only Standard or Incognito"
            );
            ensure!(
                !launch.safe_mode || launch.mode == Mode::Standard,
                "Startup Safe Mode applies only to a managed Standard profile, not Incognito"
            );
            Some((
                launch.profile_id,
                if launch.safe_mode {
                    LaunchRequest::safe_mode()
                } else {
                    LaunchRequest::new(launch.mode, None)?
                },
            ))
        }
        _ => None,
    };
    let recovery = match operation {
        Operation::Recovery => {
            let recovery: NativeRecovery = serde_json::from_slice(&bytes)
                .context("Invalid native profile recovery request")?;
            ensure!(
                recovery.protocol_version == PROTOCOL_VERSION,
                "Unsupported native profile recovery protocol"
            );
            match &recovery.request {
                RecoveryOperation::Backup { confirmed, .. }
                | RecoveryOperation::Restore { confirmed, .. } => ensure!(
                    *confirmed,
                    "Backup and clone-restore require explicit confirmation"
                ),
                RecoveryOperation::List { .. } => {}
            }
            if let RecoveryOperation::Restore { label, .. } = &recovery.request {
                crate::validate_label(label)?;
            }
            Some(recovery.request)
        }
        _ => None,
    };
    let store = Store::at_home(home)?;
    let exclusive = matches!(
        recovery,
        Some(RecoveryOperation::Backup { .. } | RecoveryOperation::Restore { .. })
    );
    let _installation = store.installation_lock(exclusive)?;
    let (app, browser) = app::identity(app_path)?;
    let packaged = fs::canonicalize(app.join(HELPER_PATH))?;
    ensure!(
        packaged.starts_with(&app) && fs::canonicalize(executable)? == packaged,
        "Managed profiles require this installed app's bundled helper; no developer or cached runner fallback is allowed"
    );
    let helper_info = plist::Value::from_file(
        app.join("Contents/Helpers/Stainless Updater.app/Contents/Info.plist"),
    )
    .context("Cannot read the bundled native profile runtime manifest")?;
    let helper_info = helper_info
        .as_dictionary()
        .context("Invalid native profile runtime manifest")?;
    ensure!(
        helper_info
            .get("CFBundleIdentifier")
            .and_then(plist::Value::as_string)
            == Some("dev.stainless.browser.updater")
            && helper_info
                .get("StainlessProfileChooserVersion")
                .and_then(plist::Value::as_unsigned_integer)
                == Some(u64::from(PROTOCOL_VERSION)),
        "This app does not support native profile chooser version 1; install a compatible browser"
    );
    if recovery.is_some() {
        ensure!(
            helper_info
                .get("StainlessProfileRecoveryVersion")
                .and_then(plist::Value::as_unsigned_integer)
                == Some(u64::from(RECOVERY_VERSION)),
            "This app does not support native profile recovery version 1; install a compatible browser"
        );
    }
    app::require_managed_modes(&app)?;
    if launch
        .as_ref()
        .is_some_and(|(_, request)| request.is_safe_mode())
    {
        ensure!(
            helper_info
                .get("StainlessProfileSafeModeVersion")
                .and_then(plist::Value::as_unsigned_integer)
                == Some(u64::from(SAFE_MODE_VERSION)),
            "This helper does not support startup Safe Mode version 1; install a compatible app. No normal-launch fallback is used"
        );
        app::require_managed_safe_mode(&app)?;
    }
    store.initialize_locked()?;
    if let Some(request) = metadata {
        write_reply(
            output,
            Reply {
                metadata: Some(store.metadata(request)?),
                ..Reply::new("metadata", "Local profile metadata.")
            },
        )?;
    }
    if let Some(request) = recovery {
        match request {
            RecoveryOperation::List { after } => write_reply(
                output,
                Reply {
                    catalog: Some(store.backup_catalog(&browser, after.as_ref())?),
                    ..Reply::new(
                        "backups",
                        "Backup metadata only; file integrity is verified on restore.",
                    )
                },
            )?,
            RecoveryOperation::Backup { profile_id, .. } => {
                let id = store.backup_locked(&profile_id)?;
                let committed = id.to_string();
                write_reply(
                    output,
                    Reply {
                        backup_id: Some(id),
                        ..Reply::new("backed_up", "Verified cold backup committed. The source profile was not changed.")
                    },
                ).with_context(|| format!(
                    "Backup {committed} was committed but its acknowledgement failed; refresh backups before retrying"
                ))?;
            }
            RecoveryOperation::Restore {
                backup_id, label, ..
            } => {
                let profile = store.restore_locked(&backup_id, &label, Some(&browser))?;
                let committed = profile.id.to_string();
                write_reply(
                    output,
                    Reply {
                        metadata: Some(MetadataResponse {
                            protocol_version: crate::protocol::PROTOCOL_VERSION,
                            registry_version: REGISTRY_VERSION,
                            profiles: vec![profile],
                        }),
                        ..Reply::new("restored", "A new managed identity was registered. Existing roots and backup were retained; no app or Keychain item was restored.")
                    },
                ).with_context(|| format!(
                    "Restored profile {committed} was registered but its acknowledgement failed; refresh profiles before retrying"
                ))?;
            }
        }
    }
    if let Some((id, request)) = launch {
        store.launch_observed(&id, &app, &request, || {
            write_reply(
                output,
                Reply {
                    safe_mode: request.is_safe_mode().then_some(true),
                    ..Reply::new(
                        "started",
                        if request.is_safe_mode() {
                            "Safe Mode process started: extensions/background apps and previous-session restoration are off for this launch. Saved configuration/session data are retained. Process creation is not profile health."
                        } else {
                            "The owned browser process started. This is not a browser health confirmation."
                        },
                    )
                },
            )
        }).with_context(|| if request.is_safe_mode() {
            "Safe Mode did not complete successfully. Profile data and its compatibility floor were retained. Close any remaining browser normally, then retry or use Backup / Restore to recover a new identity"
        } else {
            "Managed browser launch did not complete successfully"
        })?;
        write_reply(
            output,
            Reply {
                safe_mode: request.is_safe_mode().then_some(true),
                ..Reply::new(
                    "exited",
                    if request.is_safe_mode() {
                        "Safe Mode browser closed; ownership released. Choose Open Standard to return to normal startup. Previous-session data were retained; Safe Mode tabs were not saved for restoration."
                    } else {
                        "Managed browser closed; ownership released."
                    },
                )
            },
        )?;
    }
    Ok(())
}
