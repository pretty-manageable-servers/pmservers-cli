use reqwest::Method;
use serde::{Deserialize, Serialize};
use url::Url;

use crate::client::{decode, Client};
use crate::core::Items;
use crate::error::Result;

/// The object storage of an account. `status` is `null` until storage is turned on.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Storage {
    #[serde(default)]
    pub status: Option<String>,
    #[serde(default)]
    pub status_message: Option<String>,
    pub endpoint: String,
    pub region: String,
    pub used_bytes: u64,
    pub limit_bytes: u64,
    pub buckets_count: u64,
    pub buckets_limit: u64,
    pub keys_count: u64,
    pub keys_limit: u64,
    /// What the usage over the free tier would cost. Nothing is charged yet.
    pub would_cost_usd: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Bucket {
    pub name: String,
    pub public_read: bool,
    /// provisioning, active, deleting or failed.
    pub status: String,
    #[serde(default)]
    pub status_message: Option<String>,
    pub used_bytes: u64,
    pub object_count: u64,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct CreateBucket {
    pub name: String,
    pub public_read: bool,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct UpdateBucket {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub public_read: Option<bool>,
}

/// The buckets of an access key: the string `all`, or a list of bucket names.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum KeyBuckets {
    Named(Vec<String>),
    All(String),
}

impl KeyBuckets {
    pub fn all() -> Self {
        Self::All("all".into())
    }

    /// `all`, or the names separated by commas.
    pub fn describe(&self) -> String {
        match self {
            KeyBuckets::All(s) => s.clone(),
            KeyBuckets::Named(names) => names.join(", "),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StorageKey {
    pub key_id: String,
    pub name: String,
    /// read_write or read_only.
    pub permission: String,
    pub buckets: KeyBuckets,
    pub created_at: String,
    pub updated_at: String,
}

/// The answer to create and reset. `secret_access_key` is shown one time.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StorageKeyWithSecret {
    pub key_id: String,
    pub name: String,
    pub permission: String,
    pub buckets: KeyBuckets,
    pub created_at: String,
    pub updated_at: String,
    pub endpoint: String,
    pub region: String,
    pub secret_access_key: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct CreateStorageKey {
    pub name: String,
    pub permission: String,
    pub buckets: KeyBuckets,
}

impl Client {
    /// `account` is the owner of the storage. Pass it only for a project member.
    fn storage_url(&self, segments: &[&str], account: Option<&str>) -> Url {
        let mut url = self.url(segments);
        if let Some(account) = account {
            url.query_pairs_mut().append_pair("account_id", account);
        }
        url
    }

    pub async fn storage(&self, account: Option<&str>) -> Result<Storage> {
        decode(
            &self
                .send(
                    Method::GET,
                    self.storage_url(&["v1", "storage"], account),
                    None,
                )
                .await?,
        )
    }

    /// Asks for object storage. The reconciler sets it up after this call.
    pub async fn enable_storage(&self, account: Option<&str>) -> Result<Storage> {
        decode(
            &self
                .send(
                    Method::POST,
                    self.storage_url(&["v1", "storage", "enable"], account),
                    None,
                )
                .await?,
        )
    }

    pub async fn disable_storage(&self, account: Option<&str>) -> Result<Storage> {
        decode(
            &self
                .send(
                    Method::POST,
                    self.storage_url(&["v1", "storage", "disable"], account),
                    None,
                )
                .await?,
        )
    }

    pub async fn storage_buckets(&self, account: Option<&str>) -> Result<Vec<Bucket>> {
        let bytes = self
            .send(
                Method::GET,
                self.storage_url(&["v1", "storage", "buckets"], account),
                None,
            )
            .await?;
        Ok(decode::<Items<Bucket>>(&bytes)?.items)
    }

    pub async fn create_bucket(
        &self,
        account: Option<&str>,
        body: &CreateBucket,
    ) -> Result<Bucket> {
        let body = serde_json::to_value(body).map_err(|e| crate::Error::Invalid(e.to_string()))?;
        decode(
            &self
                .send(
                    Method::POST,
                    self.storage_url(&["v1", "storage", "buckets"], account),
                    Some(body),
                )
                .await?,
        )
    }

    pub async fn set_bucket_public(
        &self,
        account: Option<&str>,
        name: &str,
        public_read: bool,
    ) -> Result<Bucket> {
        let body = serde_json::to_value(UpdateBucket {
            public_read: Some(public_read),
        })
        .map_err(|e| crate::Error::Invalid(e.to_string()))?;
        decode(
            &self
                .send(
                    Method::PATCH,
                    self.storage_url(&["v1", "storage", "buckets", name], account),
                    Some(body),
                )
                .await?,
        )
    }

    /// Only an empty bucket can be deleted. The reconciler removes it.
    pub async fn delete_bucket(&self, account: Option<&str>, name: &str) -> Result<()> {
        self.send(
            Method::DELETE,
            self.storage_url(&["v1", "storage", "buckets", name], account),
            None,
        )
        .await
        .map(drop)
    }

    pub async fn storage_keys(&self, account: Option<&str>) -> Result<Vec<StorageKey>> {
        let bytes = self
            .send(
                Method::GET,
                self.storage_url(&["v1", "storage", "keys"], account),
                None,
            )
            .await?;
        Ok(decode::<Items<StorageKey>>(&bytes)?.items)
    }

    /// The answer has the secret. The API does not return it again.
    pub async fn create_storage_key(
        &self,
        account: Option<&str>,
        body: &CreateStorageKey,
    ) -> Result<StorageKeyWithSecret> {
        let body = serde_json::to_value(body).map_err(|e| crate::Error::Invalid(e.to_string()))?;
        decode(
            &self
                .send(
                    Method::POST,
                    self.storage_url(&["v1", "storage", "keys"], account),
                    Some(body),
                )
                .await?,
        )
    }

    /// A new key ID and secret. The old key stops working. The answer has the new secret.
    pub async fn reset_storage_key(
        &self,
        account: Option<&str>,
        key_id: &str,
    ) -> Result<StorageKeyWithSecret> {
        decode(
            &self
                .send(
                    Method::POST,
                    self.storage_url(&["v1", "storage", "keys", key_id, "reset"], account),
                    None,
                )
                .await?,
        )
    }

    pub async fn delete_storage_key(&self, account: Option<&str>, key_id: &str) -> Result<()> {
        self.send(
            Method::DELETE,
            self.storage_url(&["v1", "storage", "keys", key_id], account),
            None,
        )
        .await
        .map(drop)
    }
}
