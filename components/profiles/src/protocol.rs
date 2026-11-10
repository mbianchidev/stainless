use crate::{ProfileMetadata, REGISTRY_VERSION, Store};
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};

pub const PROTOCOL_VERSION: u32 = 1;
pub const MAX_REQUEST_BYTES: usize = 16 * 1024;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MetadataRequest {
    pub protocol_version: u32,
    pub request: MetadataOperation,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "operation", rename_all = "snake_case", deny_unknown_fields)]
pub enum MetadataOperation {
    List {},
    Create { label: String },
}

#[derive(Debug, Serialize)]
#[serde(deny_unknown_fields)]
pub struct MetadataResponse {
    pub protocol_version: u32,
    pub registry_version: u32,
    pub profiles: Vec<ProfileMetadata>,
}

impl MetadataRequest {
    pub fn parse(bytes: &[u8]) -> Result<Self> {
        ensure!(
            bytes.len() <= MAX_REQUEST_BYTES,
            "Metadata request is too large"
        );
        let request: Self =
            serde_json::from_slice(bytes).context("Invalid metadata-only request")?;
        request.validate()?;
        Ok(request)
    }

    fn validate(&self) -> Result<()> {
        ensure!(
            self.protocol_version == PROTOCOL_VERSION,
            "Unsupported profile metadata protocol"
        );
        Ok(())
    }
}

impl Store {
    pub fn metadata(&self, request: MetadataRequest) -> Result<MetadataResponse> {
        request.validate()?;
        let profiles = match request.request {
            MetadataOperation::List {} => self.list()?,
            MetadataOperation::Create { label } => vec![self.create(&label)?],
        };
        Ok(MetadataResponse {
            protocol_version: PROTOCOL_VERSION,
            registry_version: REGISTRY_VERSION,
            profiles,
        })
    }
}
