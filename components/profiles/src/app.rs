use crate::BrowserIdentity;
use anyhow::{Context, Result, ensure};
use stainless_release::{BUNDLE_ID, Product};
use std::fs;
use std::path::{Path, PathBuf};

pub fn product(app: &Path) -> Result<Product> {
    let info = plist::Value::from_file(app.join("Contents/Info.plist"))
        .context("Cannot read the Stainless app manifest")?;
    let info = info
        .as_dictionary()
        .context("App manifest is not a dictionary")?;
    let text = |key| -> Result<&str> {
        info.get(key)
            .and_then(plist::Value::as_string)
            .with_context(|| format!("Missing app manifest field {key}"))
    };
    ensure!(
        text("CFBundleIdentifier")? == BUNDLE_ID,
        "Wrong application identity"
    );
    ensure!(
        text("CFBundleExecutable")? == "Stainless",
        "Wrong browser executable"
    );
    ensure!(
        text("CrProductDirName")? == "Stainless",
        "Wrong browser profile identity"
    );
    let product: Product = serde_json::from_value(serde_json::json!({
        "version": text("StainlessVersion")?,
        "build": info.get("StainlessBuild").and_then(plist::Value::as_unsigned_integer)
            .context("Missing Stainless build number")?,
        "channel": text("StainlessChannel")?,
        "profile_schema": info.get("StainlessProfileSchema").and_then(plist::Value::as_unsigned_integer)
            .context("Missing profile schema")?,
    }))?;
    product.validate()?;
    Ok(product)
}

pub(crate) fn identity(app: &Path) -> Result<(PathBuf, BrowserIdentity)> {
    ensure!(app.is_absolute(), "Use an absolute Stainless.app path");
    let app = fs::canonicalize(app).context("Cannot locate Stainless.app")?;
    ensure!(
        app.file_name().is_some_and(|name| name == "Stainless.app"),
        "Expected an explicit Stainless.app bundle"
    );
    let browser = browser(&app)?;
    Ok((app, browser))
}

pub(crate) fn require_managed_modes(app: &Path) -> Result<()> {
    let info = plist::Value::from_file(app.join("Contents/Info.plist"))?;
    ensure!(
        info.as_dictionary()
            .and_then(|dictionary| dictionary.get("StainlessManagedModesVersion"))
            .and_then(plist::Value::as_unsigned_integer)
            == Some(1),
        "This app does not support managed Incognito enforcement; use a browser built with managed modes version 1"
    );
    Ok(())
}

pub fn browser(app: &Path) -> Result<BrowserIdentity> {
    let product = product(app)?;
    let info = plist::Value::from_file(app.join("Contents/Info.plist"))?;
    let chromium_version = info
        .as_dictionary()
        .and_then(|dictionary| dictionary.get("CFBundleShortVersionString"))
        .and_then(plist::Value::as_string)
        .context("Missing Chromium version in the app manifest")?
        .to_owned();
    let browser = BrowserIdentity {
        product,
        chromium_version,
    };
    browser.validate()?;
    Ok(browser)
}
