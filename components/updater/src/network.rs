use anyhow::{Context, Result, ensure};
use reqwest::blocking::{Client, Response};
use reqwest::header::{ACCEPT, AUTHORIZATION, HeaderMap, HeaderValue};
use serde::Deserialize;
use stainless_release::{
    API_ROOT, FEED_NAME, MAX_METADATA_BYTES, Product, UpdateFeed, validate_api_url,
    validate_asset_url, validate_redirect,
};
use std::fs::File;
use std::io::{Read, Write};
use std::path::Path;
use std::time::Duration;
use url::Url;
use zeroize::Zeroizing;

const KEYCHAIN_SERVICE: &str = "dev.stainless.browser.preview-updates";
const KEYCHAIN_ACCOUNT: &str = "github-read-token";

#[derive(Clone, Debug, Deserialize)]
pub struct ReleaseAsset {
    pub name: String,
    pub url: Url,
    pub size: u64,
}

#[derive(Debug, Deserialize)]
struct GitHubRelease {
    tag_name: String,
    draft: bool,
    prerelease: bool,
    assets: Vec<ReleaseAsset>,
}

pub struct PreviewClient {
    client: Client,
    token: Zeroizing<String>,
}

pub fn token() -> Result<Zeroizing<String>> {
    #[cfg(target_os = "macos")]
    {
        let bytes = Zeroizing::new(
            security_framework::passwords::get_generic_password(KEYCHAIN_SERVICE, KEYCHAIN_ACCOUNT)
                .context("Private updates need a read-only GitHub token. Configure it from the Stainless Updates menu; browsing remains available without it")?
        );
        Ok(Zeroizing::new(
            String::from_utf8(bytes.to_vec()).context("Keychain credential is not UTF-8")?,
        ))
    }
    #[cfg(not(target_os = "macos"))]
    anyhow::bail!("Automatic updates currently require the macOS Keychain")
}

pub fn save_token(value: &str) -> Result<()> {
    ensure!(
        !value.is_empty() && value.len() <= 4096 && !value.contains(char::is_whitespace),
        "Expected a nonempty scoped GitHub token, without whitespace"
    );
    #[cfg(target_os = "macos")]
    security_framework::passwords::set_generic_password(
        KEYCHAIN_SERVICE,
        KEYCHAIN_ACCOUNT,
        value.as_bytes(),
    )
    .context("Cannot save update authorization in the macOS Keychain")?;
    #[cfg(not(target_os = "macos"))]
    anyhow::bail!("Automatic updates currently require the macOS Keychain");
    Ok(())
}

pub fn delete_token() -> Result<()> {
    #[cfg(target_os = "macos")]
    security_framework::passwords::delete_generic_password(KEYCHAIN_SERVICE, KEYCHAIN_ACCOUNT)
        .context("Cannot remove the update credential from the macOS Keychain")?;
    #[cfg(not(target_os = "macos"))]
    anyhow::bail!("Automatic updates currently require the macOS Keychain");
    Ok(())
}

impl PreviewClient {
    fn client_builder() -> reqwest::blocking::ClientBuilder {
        Client::builder()
            .https_only(true)
            .connect_timeout(Duration::from_secs(20))
            .timeout(Duration::from_secs(3600))
            .redirect(reqwest::redirect::Policy::custom(|attempt| {
                if attempt.previous().len() >= 5 || validate_redirect(attempt.url()).is_err() {
                    attempt.error("Untrusted or excessive update redirect")
                } else {
                    attempt.follow()
                }
            }))
            .user_agent("Stainless-Preview-Updater/1")
    }

    pub fn new(token: Zeroizing<String>) -> Result<Self> {
        let client = Self::client_builder()
            .build()
            .context("Cannot initialize validated HTTPS networking")?;
        Ok(Self { client, token })
    }

    fn get(&self, url: &Url, binary: bool) -> Result<Response> {
        validate_api_url(url)?;
        if binary {
            validate_asset_url(url)?;
        }
        let mut headers = HeaderMap::new();
        let mut authorization = HeaderValue::from_str(&format!("Bearer {}", self.token.as_str()))
            .context("Invalid update authorization")?;
        authorization.set_sensitive(true);
        headers.insert(AUTHORIZATION, authorization);
        headers.insert(
            ACCEPT,
            HeaderValue::from_static(if binary {
                "application/octet-stream"
            } else {
                "application/vnd.github+json"
            }),
        );
        let response = self.client.get(url.clone()).headers(headers).send()
            .map_err(|_| anyhow::anyhow!("HTTPS request failed. Check connectivity, system certificates, and release access"))?;
        let status = response.status();
        ensure!(
            status.is_success(),
            "Update server returned HTTP {}. Check the token's read-only Contents permission and repository access; browsing is unaffected",
            status.as_u16()
        );
        Ok(response)
    }

    pub fn discover(
        &self,
        current: &Product,
        os_version: &str,
    ) -> Result<Option<(UpdateFeed, ReleaseAsset)>> {
        let url = Url::parse(&format!("{API_ROOT}/releases?per_page=100"))?;
        let releases: Vec<GitHubRelease> = bounded_json(self.get(&url, false)?)?;
        for release in releases {
            if release.draft
                || !release.prerelease
                || !release.tag_name.starts_with("stainless-preview-v")
            {
                continue;
            }
            let version = release.tag_name.trim_start_matches("stainless-preview-v");
            let Ok(version) = version.parse::<semver::Version>() else {
                continue;
            };
            if version <= current.version {
                continue;
            }
            let metadata = release
                .assets
                .iter()
                .find(|asset| asset.name == FEED_NAME)
                .context(
                    "Newer preview is missing its update feed; publisher must repair the release",
                )?;
            ensure!(
                metadata.size <= MAX_METADATA_BYTES,
                "Update feed is too large"
            );
            let feed: UpdateFeed = bounded_json(self.get(&metadata.url, true)?)?;
            feed.validate_upgrade(current, os_version)?;
            ensure!(
                feed.release_tag == release.tag_name,
                "Feed belongs to a different release"
            );
            let asset = release
                .assets
                .into_iter()
                .find(|asset| asset.name == feed.asset.name)
                .context("Update payload is missing from the release")?;
            validate_asset_url(&asset.url)?;
            ensure!(
                asset.size == feed.asset.size,
                "Release asset size does not match the feed"
            );
            return Ok(Some((feed, asset)));
        }
        Ok(None)
    }

    pub fn download(&self, asset: &ReleaseAsset, destination: &Path) -> Result<()> {
        validate_asset_url(&asset.url)?;
        let mut response = self.get(&asset.url, true)?.take(asset.size + 1);
        let mut file = File::options()
            .write(true)
            .create_new(true)
            .open(destination)?;
        let count = std::io::copy(&mut response, &mut file)
            .context("Update download interrupted; the installed browser has not been changed")?;
        ensure!(
            count == asset.size,
            "Update download was truncated or exceeded the declared size"
        );
        file.flush()?;
        file.sync_all()?;
        Ok(())
    }
}

fn bounded_json<T: serde::de::DeserializeOwned>(response: Response) -> Result<T> {
    let mut data = Vec::new();
    response
        .take(MAX_METADATA_BYTES + 1)
        .read_to_end(&mut data)
        .context("Metadata download interrupted")?;
    ensure!(
        data.len() as u64 <= MAX_METADATA_BYTES,
        "Update metadata exceeds the size limit"
    );
    serde_json::from_slice(&data).context("Malformed or incomplete update metadata")
}

#[cfg(all(test, target_os = "macos"))]
mod tests;
