use anyhow::{Context, Result, bail, ensure};
use base64::{Engine, engine::general_purpose::STANDARD};
use reqwest::blocking::Client;
use serde::Deserialize;
use stainless_release::{MAX_METADATA_BYTES, is_hex};
use std::cmp::Ordering;
use std::collections::BTreeMap;
use std::io::Read;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const METADATA_URL: &str =
    "https://chromiumdash.appspot.com/fetch_releases?channel=Stable&platform=Mac&num=1";

pub(super) fn check(version: &str, revision: &str) -> Result<()> {
    let checked_at = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs();
    eprintln!("Official Mac Stable check (Unix seconds {checked_at}): {METADATA_URL}");
    let client = Client::builder()
        .https_only(true)
        .connect_timeout(Duration::from_secs(10))
        .timeout(Duration::from_secs(30))
        .redirect(reqwest::redirect::Policy::limited(5))
        .build()?;
    let latest = StableRelease::from_metadata(&download(&client, METADATA_URL)?)?;
    let source_url = format!(
        "https://chromium.googlesource.com/chromium/src/+/{}/chrome/VERSION?format=TEXT",
        latest.hashes.chromium
    );
    eprintln!(
        "Pinned: {version} ({revision}); official Mac Stable: {} ({}).\nSource: {source_url}",
        latest.version, latest.hashes.chromium
    );
    let encoded = download(&client, &source_url)?;
    let source = STANDARD
        .decode(encoded.trim_ascii())
        .context("Official Chromium source VERSION is not valid base64")?;
    latest.check_source_version(
        std::str::from_utf8(&source).context("Official Chromium source VERSION is not UTF-8")?,
    )?;
    latest.check_pin(version, revision)?;
    println!("Pinned Chromium matches verified official Mac Stable: {version} ({revision})");
    Ok(())
}

fn download(client: &Client, url: &str) -> Result<Vec<u8>> {
    let response = client
        .get(url)
        .send()
        .with_context(|| format!("Official Chromium evidence unavailable: {url}"))?
        .error_for_status()
        .with_context(|| format!("Official Chromium evidence returned an HTTP error: {url}"))?;
    let mut bytes = Vec::new();
    response
        .take(MAX_METADATA_BYTES + 1)
        .read_to_end(&mut bytes)
        .context("Official Chromium evidence download was interrupted")?;
    ensure!(
        bytes.len() as u64 <= MAX_METADATA_BYTES,
        "Official Chromium evidence exceeds the size limit"
    );
    Ok(bytes)
}

#[derive(Debug, Deserialize)]
struct StableRelease {
    channel: String,
    platform: String,
    version: String,
    hashes: Hashes,
}

#[derive(Debug, Deserialize)]
struct Hashes {
    chromium: String,
}

impl StableRelease {
    fn from_metadata(bytes: &[u8]) -> Result<Self> {
        ensure!(
            bytes.len() as u64 <= MAX_METADATA_BYTES,
            "Official Chromium release metadata exceeds the size limit"
        );
        let releases: Vec<Self> = serde_json::from_slice(bytes)
            .context("Malformed official Chromium release metadata")?;
        let release = releases
            .into_iter()
            .next()
            .context("Official Mac Stable release metadata is empty")?;
        ensure!(
            release.channel == "Stable" && release.platform == "Mac",
            "Official metadata did not identify Mac Stable"
        );
        version_components(&release.version)?;
        ensure!(
            is_hex(&release.hashes.chromium, 40),
            "Official Mac Stable revision must be a full lowercase hexadecimal hash"
        );
        Ok(release)
    }

    fn check_source_version(&self, source: &str) -> Result<()> {
        let names = ["MAJOR", "MINOR", "BUILD", "PATCH"];
        let mut fields = BTreeMap::new();
        for line in source.lines() {
            let (name, value) = line
                .split_once('=')
                .context("Malformed official Chromium source VERSION")?;
            ensure!(
                names.contains(&name) && fields.insert(name, value).is_none(),
                "Unknown or duplicate official Chromium source VERSION field"
            );
        }
        let source_version = names
            .iter()
            .map(|name| {
                fields
                    .get(name)
                    .copied()
                    .context("Incomplete official Chromium source VERSION")
            })
            .collect::<Result<Vec<_>>>()?
            .join(".");
        ensure!(
            version_components(&source_version)? == version_components(&self.version)?,
            "Official Chromium source VERSION {source_version} disagrees with Mac Stable metadata {}",
            self.version
        );
        Ok(())
    }

    fn check_pin(&self, version: &str, revision: &str) -> Result<()> {
        ensure!(is_hex(revision, 40), "Invalid pinned Chromium revision");
        let pinned = version_components(version)?;
        let latest = version_components(&self.version)?;
        match pinned.cmp(&latest) {
            Ordering::Less => bail!(
                "Pinned Chromium {version} ({revision}) is superseded by official Mac Stable {} ({}). Rebase and review upstream security fixes before release",
                self.version,
                self.hashes.chromium
            ),
            Ordering::Greater => bail!(
                "Pinned Chromium {version} ({revision}) is newer than official Mac Stable {} ({}). Resolve the upstream provenance discrepancy before release; do not downgrade the pin",
                self.version,
                self.hashes.chromium
            ),
            Ordering::Equal => ensure!(
                revision == self.hashes.chromium,
                "Chromium {} has inconsistent provenance: pinned revision {revision}, official Mac Stable revision {}. Resolve before release",
                self.version,
                self.hashes.chromium
            ),
        }
        Ok(())
    }
}

fn version_components(version: &str) -> Result<[u32; 4]> {
    let parts: Vec<_> = version.split('.').collect();
    ensure!(
        parts.len() == 4
            && parts
                .iter()
                .all(|part| { !part.is_empty() && part.bytes().all(|byte| byte.is_ascii_digit()) }),
        "Chromium version must contain four numeric components"
    );
    let mut result = [0; 4];
    for (index, part) in parts.iter().enumerate() {
        result[index] = part
            .parse()
            .context("Chromium version component overflow")?;
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn metadata() -> Vec<u8> {
        serde_json::to_vec(&serde_json::json!([{
            "channel": "Stable",
            "platform": "Mac",
            "version": "158.0.8127.18",
            "hashes": { "chromium": "a".repeat(40) }
        }]))
        .unwrap()
    }

    #[test]
    fn compares_the_complete_pin_and_refuses_to_downgrade_a_newer_baseline() {
        let release = StableRelease::from_metadata(&metadata()).unwrap();
        release.check_pin("158.0.8127.18", &"a".repeat(40)).unwrap();
        let newer = release
            .check_pin("159.0.9000.1", &"b".repeat(40))
            .unwrap_err()
            .to_string();
        assert!(newer.contains("newer than official Mac Stable"), "{newer}");
        assert!(newer.contains("do not downgrade"), "{newer}");
        let older = release
            .check_pin("158.0.8127.9", &"b".repeat(40))
            .unwrap_err()
            .to_string();
        assert!(older.contains("superseded"), "{older}");
        assert!(release.check_pin("158.0.8127.17", &"a".repeat(40)).is_err());
        let mismatch = release
            .check_pin("158.0.8127.18", &"b".repeat(40))
            .unwrap_err()
            .to_string();
        assert!(mismatch.contains("inconsistent provenance"), "{mismatch}");
    }

    #[test]
    fn requires_a_complete_unambiguous_official_source_version() {
        let release = StableRelease::from_metadata(&metadata()).unwrap();
        release
            .check_source_version("MAJOR=158\nMINOR=0\nBUILD=8127\nPATCH=18\n")
            .unwrap();
        for source in [
            "MAJOR=158\nMINOR=0\nBUILD=8127\nPATCH=19\n",
            "MAJOR=158\nMINOR=0\nBUILD=8127\n",
            "MAJOR=158\nMINOR=0\nBUILD=8127\nPATCH=18\nPATCH=19\n",
            "MAJOR=158\nMINOR=0\nBUILD=8127\nPATCH=+18\n",
        ] {
            assert!(release.check_source_version(source).is_err(), "{source}");
        }
    }

    #[test]
    fn refuses_missing_wrong_channel_or_oversized_upstream_evidence() {
        for value in [
            serde_json::json!([]),
            serde_json::json!({}),
            serde_json::json!([{
                "channel": "Beta", "platform": "Mac", "version": "158.0.8127.18",
                "hashes": { "chromium": "a".repeat(40) }
            }]),
            serde_json::json!([{
                "channel": "Stable", "platform": "Windows", "version": "158.0.8127.18",
                "hashes": { "chromium": "a".repeat(40) }
            }]),
            serde_json::json!([{
                "channel": "Stable", "platform": "Mac", "version": "158.0.8127",
                "hashes": { "chromium": "a".repeat(40) }
            }]),
            serde_json::json!([{
                "channel": "Stable", "platform": "Mac", "version": "158.0.8127.18",
                "hashes": { "chromium": "a".repeat(39) }
            }]),
        ] {
            assert!(StableRelease::from_metadata(&serde_json::to_vec(&value).unwrap()).is_err());
        }
        let mut bytes = metadata();
        bytes.resize(MAX_METADATA_BYTES as usize, b' ');
        StableRelease::from_metadata(&bytes).unwrap();
        bytes.push(b' ');
        assert!(StableRelease::from_metadata(&bytes).is_err());
    }
}
