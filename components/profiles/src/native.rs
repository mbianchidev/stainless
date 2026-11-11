use crate::modes::{LaunchRequest, Mode};
use crate::protocol::{MAX_REQUEST_BYTES, MetadataRequest, MetadataResponse};
use crate::{ProfileId, Store, app};
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use std::fs;
use std::io::{Read, Write};
use std::path::Path;

pub const PROTOCOL_VERSION: u32 = 1;
pub const HELPER_PATH: &str =
    "Contents/Helpers/Stainless Updater.app/Contents/Resources/stainless-update";

#[derive(Clone, Copy)]
pub enum Operation {
    Metadata,
    Launch,
}

impl Operation {
    pub fn from_command(command: &str) -> Option<Self> {
        match command {
            "profile-metadata" => Some(Self::Metadata),
            "profile-launch" => Some(Self::Launch),
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
}

#[derive(Serialize)]
struct Reply<'a> {
    protocol_version: u32,
    state: &'a str,
    message: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    metadata: Option<MetadataResponse>,
}

fn report(output: &mut impl Write, state: &str, message: &str) -> Result<()> {
    write_reply(
        output,
        Reply {
            protocol_version: PROTOCOL_VERSION,
            state,
            message,
            metadata: None,
        },
    )
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
        Operation::Launch => None,
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
            Some((launch.profile_id, LaunchRequest::new(launch.mode, None)?))
        }
        Operation::Metadata => None,
    };
    let store = Store::at_home(home)?;
    let _installation = store.installation_lock(false)?;
    let (app, _) = app::identity(app_path)?;
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
    app::require_managed_modes(&app)?;
    store.initialize()?;
    if let Some(request) = metadata {
        write_reply(
            output,
            Reply {
                protocol_version: PROTOCOL_VERSION,
                state: "metadata",
                message: "Local profile metadata.",
                metadata: Some(store.metadata(request)?),
            },
        )?;
    }
    if let Some((id, request)) = launch {
        store.launch_observed(&id, &app, &request, || {
            report(
                output,
                "started",
                "The owned browser process started. This is not a browser health confirmation.",
            )
        })?;
        report(
            output,
            "exited",
            "Managed browser closed; ownership released.",
        )?;
    }
    Ok(())
}
