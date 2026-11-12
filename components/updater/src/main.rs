use anyhow::{Context, Result, ensure};
use fs2::FileExt;
use stainless_profiles::native;
use stainless_updater::{Locations, Status, bundle, install, network, write_json};
use std::io::Read;
use std::path::Path;
use zeroize::Zeroizing;

fn main() {
    if let Some(operation) = std::env::args_os()
        .nth(1)
        .as_deref()
        .and_then(std::ffi::OsStr::to_str)
        .and_then(native::Operation::from_command)
    {
        if let Err(error) = native_profiles(operation) {
            if let Err(report_error) = native::report_error(&mut std::io::stdout(), &error) {
                eprintln!("Cannot report native profile failure: {report_error:#}");
            }
            eprintln!("Stainless profiles: {error:#}");
            std::process::exit(1);
        }
        return;
    }
    match execute() {
        Ok(status) => {
            if let Err(error) = serde_json::to_writer(std::io::stdout(), &status) {
                eprintln!("Cannot report updater status: {error}");
                std::process::exit(1);
            }
        }
        Err(error) => {
            let status = Status::new("error", format!("{error:#}"));
            if let Err(report_error) = serde_json::to_writer(std::io::stdout(), &status) {
                eprintln!("Cannot report updater failure: {report_error}");
            }
            eprintln!("Stainless updater: {error:#}");
            std::process::exit(1);
        }
    }
}

fn native_profiles(operation: native::Operation) -> Result<()> {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    ensure!(
        args.len() == 3 && args[1] == "--app",
        "Native profile operations require only --app and one typed request on stdin"
    );
    let home = std::env::var_os("HOME").context("HOME is unset")?;
    native::execute(
        operation,
        Path::new(&args[2]),
        &std::env::current_exe()?,
        Path::new(&home),
        std::io::stdin(),
        &mut std::io::stdout(),
    )
}

fn execute() -> Result<Status> {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    ensure!(
        args.len() >= 3 && args[1] == "--app",
        "Usage: stainless-update COMMAND --app /Applications/Stainless.app [--observed-start]"
    );
    let operation = args[0].to_str().context("Invalid operation")?;
    ensure!(
        args.len() == 3
            || (operation == "reconcile" && args.len() == 4 && args[3] == "--observed-start"),
        "Unexpected updater arguments"
    );
    let locations = Locations::for_app(Path::new(&args[2]))?;
    let lock = locations.lock("state.lock")?;
    lock.try_lock_exclusive()
        .context("Another updater operation is already in progress")?;
    let result = (|| -> Result<Status> {
        Ok(match operation {
            "authorize" => {
                let mut input = Zeroizing::new(String::new());
                std::io::stdin().take(4097).read_to_string(&mut input)?;
                network::save_token(input.trim())?;
                Status::new(
                    "authorized",
                    "Read-only release authorization saved in the macOS Keychain.",
                )
            }
            "forget" => {
                network::delete_token()?;
                Status::new(
                    "unauthorized",
                    "Private update authorization removed. Browsing is unaffected.",
                )
            }
            "status" => install::status(&locations)?,
            "check" => {
                let current = bundle::product(&locations.app)?;
                let client = network::PreviewClient::new(network::token()?)?;
                if let Some((feed, asset)) = client.discover(&current, &install::os_version()?)? {
                    install::reject_failed_build(&locations, feed.release.build)?;
                    let download = tempfile::tempdir_in(&locations.state)?;
                    let payload = download.path().join("update.zip");
                    client.download(&asset, &payload)?;
                    install::stage(&locations, &feed, &payload)?
                } else {
                    Status::new("current", "No newer preview is available.")
                }
            }
            "install" => install::install(&locations)?,
            "reconcile" => install::reconcile(
                &locations,
                args.get(3).is_some_and(|arg| arg == "--observed-start"),
            )?,
            _ => anyhow::bail!("Unknown updater operation"),
        })
    })();
    let status = match result {
        Ok(status) => status,
        Err(error) => {
            if !matches!(operation, "status" | "reconcile") {
                write_json(
                    &locations.state.join("status.json"),
                    &Status::new("error", format!("{error:#}")),
                )
                .context("Update failed and its error status could not be saved")?;
            }
            return Err(error);
        }
    };
    if operation != "status" {
        write_json(&locations.state.join("status.json"), &status)?;
    }
    Ok(status)
}
