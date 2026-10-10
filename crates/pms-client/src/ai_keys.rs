use serde::{Deserialize, Serialize};

use crate::client::Client;
use crate::core::Items;
use crate::error::Result;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AiKey {
    pub id: String,
    pub name: String,
    pub status: String,
    /// The start of the key, to tell keys apart.
    pub prefix: String,
    pub models: Vec<String>,
    pub budget_usd: f64,
    pub spend_usd: f64,
    #[serde(default)]
    pub rate_limit_rpm: Option<i64>,
    pub created_at: String,
    /// Only in the answer to create and rotate. It is shown one time.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub secret: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct CreateAiKey {
    pub name: String,
    pub models: Vec<String>,
    pub budget_usd: f64,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct UpdateAiKey {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub models: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub budget_usd: Option<f64>,
}

impl Client {
    pub async fn ai_keys(&self, project: &str) -> Result<Vec<AiKey>> {
        Ok(self
            .get::<Items<AiKey>>(&["v1", "projects", project, "ai-keys"])
            .await?
            .items)
    }

    pub async fn create_ai_key(&self, project: &str, body: &CreateAiKey) -> Result<AiKey> {
        self.post(&["v1", "projects", project, "ai-keys"], body)
            .await
    }

    pub async fn ai_key(&self, project: &str, id: &str) -> Result<AiKey> {
        self.get(&["v1", "projects", project, "ai-keys", id]).await
    }

    pub async fn update_ai_key(
        &self,
        project: &str,
        id: &str,
        body: &UpdateAiKey,
    ) -> Result<AiKey> {
        self.patch(&["v1", "projects", project, "ai-keys", id], body)
            .await
    }

    /// The old secret stops working. The answer has the new `secret`.
    pub async fn rotate_ai_key(&self, project: &str, id: &str) -> Result<AiKey> {
        self.post_empty(&["v1", "projects", project, "ai-keys", id, "rotate"])
            .await
    }

    pub async fn delete_ai_key(&self, project: &str, id: &str) -> Result<()> {
        self.delete(&["v1", "projects", project, "ai-keys", id])
            .await
    }
}
