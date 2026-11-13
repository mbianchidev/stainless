use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};
use std::str::FromStr;
use url::Url;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Mode {
    Standard,
    Incognito,
    Guest,
    PrivacyContainer,
    Developer,
    AgentSandbox,
    TorProxy,
    HardenedOnion,
}

impl Mode {
    pub const ALL: [Self; 8] = [
        Self::Standard,
        Self::Incognito,
        Self::Guest,
        Self::PrivacyContainer,
        Self::Developer,
        Self::AgentSandbox,
        Self::TorProxy,
        Self::HardenedOnion,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Self::Standard => "standard",
            Self::Incognito => "incognito",
            Self::Guest => "guest",
            Self::PrivacyContainer => "privacy-container",
            Self::Developer => "developer",
            Self::AgentSandbox => "agent-sandbox",
            Self::TorProxy => "tor-proxy",
            Self::HardenedOnion => "hardened-onion",
        }
    }

    pub fn policy(self) -> ModePolicy {
        let unavailable_reason = match self {
            Self::Standard | Self::Incognito => None,
            Self::Guest => Some(
                "Memory-only storage, restricted downloads and guaranteed teardown are not implemented",
            ),
            Self::PrivacyContainer => Some(
                "Container storage/network partitioning and privacy enforcement are not implemented",
            ),
            Self::Developer => {
                Some("Workspace trust and scoped developer capabilities are not implemented")
            }
            Self::AgentSandbox => Some(
                "Disposable agent isolation, approvals and resource ceilings are not implemented",
            ),
            Self::TorProxy | Self::HardenedOnion => Some(
                "Verified Tor routing, leak prevention and failure isolation are not implemented",
            ),
        };
        ModePolicy {
            mode: self,
            unavailable_reason,
            storage: match self {
                Self::Standard => StoragePolicy::ManagedProfile,
                Self::Incognito => StoragePolicy::TemporaryRoot,
                _ => StoragePolicy::Unavailable,
            },
            identity: match self {
                Self::Standard => IdentityPolicy::ExistingLocalProfile,
                Self::Incognito => IdentityPolicy::FreshSession,
                _ => IdentityPolicy::Unavailable,
            },
            history: match self {
                Self::Standard => HistoryPolicy::ChromiumPreference,
                Self::Incognito => HistoryPolicy::NotRecorded,
                _ => HistoryPolicy::Unavailable,
            },
            extensions: match self {
                Self::Standard => ExtensionPolicy::ChromiumUserChoice,
                Self::Incognito => ExtensionPolicy::Disabled,
                _ => ExtensionPolicy::Unavailable,
            },
            local_passwords: self == Self::Standard,
            ai: false,
            agents: false,
            vault: false,
            user_directed_downloads: unavailable_reason.is_none(),
            direct_network: unavailable_reason.is_none(),
        }
    }

    pub(crate) fn require_available(self) -> Result<()> {
        if let Some(reason) = self.policy().unavailable_reason {
            anyhow::bail!("Browsing mode {} is unavailable: {reason}", self.name());
        }
        Ok(())
    }
}

impl FromStr for Mode {
    type Err = anyhow::Error;

    fn from_str(value: &str) -> Result<Self> {
        Self::ALL
            .into_iter()
            .find(|mode| mode.name() == value)
            .ok_or_else(|| anyhow::anyhow!("Unknown browsing mode; use profiles modes"))
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum StoragePolicy {
    ManagedProfile,
    TemporaryRoot,
    Unavailable,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum IdentityPolicy {
    ExistingLocalProfile,
    FreshSession,
    Unavailable,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum HistoryPolicy {
    ChromiumPreference,
    NotRecorded,
    Unavailable,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ExtensionPolicy {
    ChromiumUserChoice,
    Disabled,
    Unavailable,
}

#[derive(Debug, Serialize)]
pub struct ModePolicy {
    pub mode: Mode,
    pub unavailable_reason: Option<&'static str>,
    pub storage: StoragePolicy,
    pub identity: IdentityPolicy,
    pub history: HistoryPolicy,
    pub extensions: ExtensionPolicy,
    pub local_passwords: bool,
    pub ai: bool,
    pub agents: bool,
    pub vault: bool,
    pub user_directed_downloads: bool,
    pub direct_network: bool,
}

pub struct LaunchRequest {
    mode: Mode,
    url: Option<Url>,
    safe_mode: bool,
}

impl LaunchRequest {
    pub fn new(mode: Mode, url: Option<&str>) -> Result<Self> {
        mode.require_available()?;
        let url = url
            .map(|value| {
                ensure!(
                    value.len() <= 16 * 1024 && !value.chars().any(char::is_control),
                    "The explicitly transferred URL is invalid or too large"
                );
                let url = Url::parse(value)?;
                ensure!(
                    matches!(url.scheme(), "http" | "https")
                        && url.has_host()
                        && url.username().is_empty()
                        && url.password().is_none(),
                    "Only an explicit HTTP(S) URL without embedded credentials can be transferred"
                );
                Ok(url)
            })
            .transpose()?;
        Ok(Self {
            mode,
            url,
            safe_mode: false,
        })
    }

    pub fn safe_mode() -> Self {
        Self {
            mode: Mode::Standard,
            url: None,
            safe_mode: true,
        }
    }

    pub fn mode(&self) -> Mode {
        self.mode
    }

    pub fn is_safe_mode(&self) -> bool {
        self.safe_mode
    }

    pub(crate) fn url(&self) -> Option<&Url> {
        self.url.as_ref()
    }
}
