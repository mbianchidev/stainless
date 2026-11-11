use anyhow::{Context, Result, bail, ensure};
use semver::Version;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs::File;
use std::io::Read;
use std::path::Path;
use url::Url;

pub const BUNDLE_ID: &str = "dev.stainless.browser";
pub const REPOSITORY: &str = "mbianchidev/stainless";
pub const API_ROOT: &str = "https://api.github.com/repos/mbianchidev/stainless";
pub const FEED_NAME: &str = "stainless-preview.json";
pub const MAX_PAYLOAD_BYTES: u64 = 4 * 1024 * 1024 * 1024;
pub const MAX_METADATA_BYTES: u64 = 1024 * 1024;

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Product {
    pub version: Version,
    pub build: u64,
    pub channel: String,
    pub profile_schema: u32,
}

impl Product {
    pub fn validate(&self) -> Result<()> {
        ensure!(self.build > 0, "Build number must be positive");
        ensure!(self.profile_schema > 0, "Profile schema must be positive");
        ensure!(
            self.channel == "preview",
            "Only the preview channel is implemented"
        );
        ensure!(
            self.version.pre.as_str().starts_with("preview.") && self.version.build.is_empty(),
            "Preview versions must have the form MAJOR.MINOR.PATCH-preview.N"
        );
        let sequence = self
            .version
            .pre
            .as_str()
            .strip_prefix("preview.")
            .context("Missing preview sequence")?;
        ensure!(
            !sequence.is_empty() && sequence.bytes().all(|byte| byte.is_ascii_digit()),
            "Preview sequence must be an integer"
        );
        Ok(())
    }

    pub fn release_tag(&self) -> String {
        format!("stainless-preview-v{}", self.version)
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Asset {
    pub name: String,
    pub size: u64,
    pub sha256: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct UpdateFeed {
    pub schema_version: u32,
    pub product: String,
    pub release_tag: String,
    pub release: Product,
    pub os: String,
    pub arch: String,
    pub minimum_macos: String,
    pub chromium_revision: String,
    pub chromium_version: String,
    pub asset: Asset,
}

impl UpdateFeed {
    pub fn validate(&self) -> Result<()> {
        self.release.validate()?;
        ensure!(self.schema_version == 1, "Unsupported update feed schema");
        ensure!(self.product == "stainless", "Wrong product");
        ensure!(
            self.os == "macos" && self.arch == "arm64",
            "Wrong update target"
        );
        ensure!(
            self.release_tag == self.release.release_tag(),
            "Release tag mismatch"
        );
        ensure!(
            self.asset.name == format!("Stainless-{}-macos-arm64.zip", self.release.version),
            "Unexpected update asset name"
        );
        ensure!(
            self.asset.size > 0 && self.asset.size <= MAX_PAYLOAD_BYTES,
            "Invalid update size"
        );
        ensure!(is_hex(&self.asset.sha256, 64), "Invalid SHA-256");
        ensure!(
            is_hex(&self.chromium_revision, 40),
            "Invalid Chromium revision"
        );
        ensure!(
            self.chromium_version.split('.').count() == 4
                && self
                    .chromium_version
                    .split('.')
                    .all(|part| part.parse::<u32>().is_ok()),
            "Invalid Chromium version"
        );
        macos_version(&self.minimum_macos)?;
        Ok(())
    }

    pub fn validate_upgrade(&self, current: &Product, os_version: &str) -> Result<()> {
        self.validate()?;
        current.validate()?;
        ensure!(
            self.release.channel == current.channel,
            "Wrong update channel"
        );
        ensure!(
            self.release.build > current.build && self.release.version > current.version,
            "Stale update or downgrade"
        );
        ensure!(
            self.release.profile_schema == current.profile_schema,
            "Profile schema migration is not implemented; refusing this update"
        );
        ensure!(
            macos_version(os_version)? >= macos_version(&self.minimum_macos)?,
            "Update requires a newer macOS version"
        );
        Ok(())
    }
}

pub fn macos_version(value: &str) -> Result<(u32, u32, u32)> {
    let parts: Vec<_> = value.split('.').collect();
    ensure!((2..=3).contains(&parts.len()), "Invalid macOS version");
    ensure!(
        parts
            .iter()
            .all(|part| !part.is_empty() && part.bytes().all(|byte| byte.is_ascii_digit())),
        "Invalid macOS version"
    );
    let major = parts[0].parse().context("Invalid macOS major version")?;
    let minor = parts[1].parse().context("Invalid macOS minor version")?;
    let patch = if parts.len() == 3 {
        parts[2].parse().context("Invalid macOS patch version")?
    } else {
        0
    };
    Ok((major, minor, patch))
}

pub fn is_hex(value: &str, size: usize) -> bool {
    value.len() == size
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

pub fn sha256(path: &Path) -> Result<String> {
    let input = File::open(path).with_context(|| format!("Cannot read {}", path.display()))?;
    sha256_reader(input)
}

pub fn sha256_reader(mut input: impl Read) -> Result<String> {
    let mut digest = Sha256::new();
    let mut buffer = [0; 128 * 1024];
    loop {
        let count = input.read(&mut buffer).context("Cannot hash file")?;
        if count == 0 {
            break;
        }
        digest.update(&buffer[..count]);
    }
    Ok(digest
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect())
}

pub fn verify_asset(path: &Path, asset: &Asset) -> Result<()> {
    ensure!(
        path.metadata()?.len() == asset.size,
        "Update payload size mismatch"
    );
    ensure!(
        sha256(path)? == asset.sha256,
        "Update payload SHA-256 mismatch"
    );
    Ok(())
}

pub fn validate_api_url(url: &Url) -> Result<()> {
    ensure!(
        url.scheme() == "https"
            && url.host_str() == Some("api.github.com")
            && url.port_or_known_default() == Some(443)
            && url.username().is_empty()
            && url.password().is_none()
            && url.fragment().is_none(),
        "Untrusted GitHub API source"
    );
    ensure!(
        url.path() == "/repos/mbianchidev/stainless/releases"
            || url
                .path()
                .starts_with("/repos/mbianchidev/stainless/releases/"),
        "Wrong update repository"
    );
    Ok(())
}

pub fn validate_asset_url(url: &Url) -> Result<()> {
    validate_api_url(url)?;
    let prefix = "/repos/mbianchidev/stainless/releases/assets/";
    let id = url
        .path()
        .strip_prefix(prefix)
        .context("Expected release asset API URL")?;
    ensure!(
        !id.is_empty() && id.bytes().all(|byte| byte.is_ascii_digit()) && url.query().is_none(),
        "Invalid release asset URL"
    );
    Ok(())
}

pub fn validate_redirect(url: &Url) -> Result<()> {
    ensure!(
        url.scheme() == "https"
            && url.port_or_known_default() == Some(443)
            && url.username().is_empty()
            && url.password().is_none()
            && url.fragment().is_none(),
        "Update redirect must use validated HTTPS"
    );
    match url.host_str() {
        Some("api.github.com") => validate_api_url(url),
        Some("release-assets.githubusercontent.com" | "objects.githubusercontent.com") => Ok(()),
        _ => bail!("Update redirect host is not trusted"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn product(build: u64) -> Product {
        Product {
            version: Version::parse(&format!("0.1.0-preview.{build}")).unwrap(),
            build,
            channel: "preview".into(),
            profile_schema: 1,
        }
    }

    fn feed() -> UpdateFeed {
        UpdateFeed {
            schema_version: 1,
            product: "stainless".into(),
            release_tag: product(2).release_tag(),
            release: product(2),
            os: "macos".into(),
            arch: "arm64".into(),
            minimum_macos: "13.0".into(),
            chromium_revision: "a".repeat(40),
            chromium_version: "156.0.8000.1".into(),
            asset: Asset {
                name: "Stainless-0.1.0-preview.2-macos-arm64.zip".into(),
                size: 3,
                sha256: "a".repeat(64),
            },
        }
    }

    #[test]
    fn accepts_matching_newer_preview() {
        feed().validate_upgrade(&product(1), "26.6").unwrap();
    }

    #[test]
    fn rejects_repeated_preview_prefixes() {
        let mut release = product(1);
        release.version = Version::parse("0.1.0-preview.preview.1").unwrap();
        assert!(release.validate().is_err());
    }

    #[test]
    fn rejects_same_version_and_downgrades() {
        for build in [2, 3] {
            assert!(feed().validate_upgrade(&product(build), "26.6").is_err());
        }
    }

    #[test]
    fn rejects_lower_build_even_with_newer_semver() {
        let mut current = product(3);
        current.version = Version::parse("0.0.1-preview.1").unwrap();
        assert!(feed().validate_upgrade(&current, "26.6").is_err());
    }

    #[test]
    fn rejects_profile_schema_changes_before_migrations_exist() {
        let mut update = feed();
        update.release.profile_schema = 2;
        assert!(update.validate_upgrade(&product(1), "26.6").is_err());
    }

    #[test]
    fn rejects_wrong_targets_and_channels() {
        let mut update = feed();
        update.arch = "x86_64".into();
        assert!(update.validate().is_err());
        let mut update = feed();
        update.release.channel = "stable".into();
        assert!(update.validate().is_err());
    }

    #[test]
    fn rejects_missing_or_unknown_feed_fields() {
        let mut json = serde_json::to_value(feed()).unwrap();
        json.as_object_mut().unwrap().remove("asset");
        assert!(serde_json::from_value::<UpdateFeed>(json).is_err());
        let mut json = serde_json::to_value(feed()).unwrap();
        json["unexpected"] = true.into();
        assert!(serde_json::from_value::<UpdateFeed>(json).is_err());
    }

    #[test]
    fn rejects_inconsistent_release_and_asset_names() {
        let mut update = feed();
        update.release_tag = "stainless-preview-v0.1.0-preview.99".into();
        assert!(update.validate().is_err());
        let mut update = feed();
        update.asset.name = "../Stainless.zip".into();
        assert!(update.validate().is_err());
    }

    #[test]
    fn compares_os_versions_numerically() {
        assert!(feed().validate_upgrade(&product(1), "12.7.6").is_err());
        assert!(macos_version("26.10").unwrap() > macos_version("26.6").unwrap());
        assert!(macos_version("26").is_err());
        assert!(macos_version("26.x").is_err());
    }

    #[test]
    fn rejects_corruption_and_truncated_downloads() {
        let mut file = tempfile::NamedTempFile::new().unwrap();
        file.write_all(b"abc").unwrap();
        let asset = Asset {
            name: "fixture.zip".into(),
            size: 3,
            sha256: "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad".into(),
        };
        verify_asset(file.path(), &asset).unwrap();
        file.as_file_mut().set_len(2).unwrap();
        assert!(verify_asset(file.path(), &asset).is_err());
        file.as_file_mut().set_len(3).unwrap();
        assert!(verify_asset(file.path(), &asset).is_err());
    }

    #[test]
    fn restricts_private_asset_authorization_to_this_repository() {
        validate_asset_url(&Url::parse(&format!("{API_ROOT}/releases/assets/42")).unwrap())
            .unwrap();
        for url in [
            "http://api.github.com/repos/mbianchidev/stainless/releases/assets/42",
            "https://api.github.com/repos/another/repo/releases/assets/42",
            "https://api.github.com/repos/mbianchidev/stainless/releases/assets/../42",
            "https://api.github.com/repos/mbianchidev/stainless/releases/assets/42?token=secret",
            "https://api.github.com.evil.invalid/repos/mbianchidev/stainless/releases/assets/42",
            "https://api.github.com/repos/mbianchidev/stainless/releases/assets/42/extra",
        ] {
            assert!(
                validate_asset_url(&Url::parse(url).unwrap()).is_err(),
                "{url}"
            );
        }
    }

    #[test]
    fn rejects_https_downgrades_and_untrusted_redirects() {
        for url in [
            "http://release-assets.githubusercontent.com/fixture",
            "https://release-assets.githubusercontent.com.evil.invalid/fixture",
            "https://evil.invalid/fixture",
            "https://token@release-assets.githubusercontent.com/fixture",
        ] {
            assert!(validate_redirect(&Url::parse(url).unwrap()).is_err());
        }
        validate_redirect(
            &Url::parse("https://release-assets.githubusercontent.com/fixture?opaque=download")
                .unwrap(),
        )
        .unwrap();
    }
}
