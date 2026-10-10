use serde::{Deserialize, Serialize};

use crate::client::Client;
use crate::error::Result;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Me {
    pub id: String,
    pub email: String,
    pub plan: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VmSize {
    pub id: String,
    pub vcpu: u32,
    pub memory_gib: u32,
}

/// Container and database sizes have the same shape.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComputeSize {
    pub id: String,
    pub vcpu: f64,
    pub memory_mib: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VmImage {
    pub id: String,
    pub name: String,
    pub version: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AiModel {
    pub id: String,
    #[serde(rename = "type")]
    pub kind: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub detail: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Catalog {
    #[serde(default)]
    pub regions: Vec<String>,
    #[serde(default)]
    pub vm_sizes: Vec<VmSize>,
    #[serde(default)]
    pub container_sizes: Vec<ComputeSize>,
    #[serde(default)]
    pub database_sizes: Vec<ComputeSize>,
    #[serde(default)]
    pub vm_images: Vec<VmImage>,
    #[serde(default)]
    pub ai_models: Vec<AiModel>,
}

impl Client {
    pub async fn me(&self) -> Result<Me> {
        self.get(&["v1", "me"]).await
    }

    pub async fn catalog(&self) -> Result<Catalog> {
        self.get(&["v1", "catalog"]).await
    }
}
