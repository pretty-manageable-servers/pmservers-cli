use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};
use async_trait::async_trait;
use pms_client::{Error, TokenSource};
use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;

use crate::config::config_dir;
use crate::supabase::{Supabase, SupabaseConfig, TokenResponse};

const KEYRING_SERVICE: &str = "pms-cli";
const KEYRING_USER: &str = "session";
/// Refresh this long before the token ends, so that a request does not race the expiry.
const REFRESH_MARGIN: i64 = 60;

/// `PMS_TOKEN`, an API token. It wins over the saved session and the keyring. Empty means unset.
pub fn env_token() -> Option<String> {
    std::env::var("PMS_TOKEN")
        .ok()
        .map(|v| v.trim().to_owned())
        .filter(|v| !v.is_empty())
}

pub fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoredSession {
    pub access_token: String,
    pub refresh_token: String,
    pub expires_at: i64,
    pub user_id: String,
    pub email: String,
    pub provider: String,
    pub supabase_url: String,
}

impl StoredSession {
    pub fn from_response(r: &TokenResponse, supabase_url: &str) -> Self {
        Self {
            access_token: r.access_token.clone(),
            refresh_token: r.refresh_token.clone(),
            expires_at: r.expires_at(),
            user_id: r.user.id.clone(),
            email: r.user.email.clone().unwrap_or_default(),
            provider: r
                .user
                .app_metadata
                .get("provider")
                .and_then(|v| v.as_str())
                .unwrap_or("email")
                .to_owned(),
            supabase_url: supabase_url.to_owned(),
        }
    }
}

fn file_path() -> Result<PathBuf> {
    Ok(config_dir()?.join("credentials.json"))
}

fn use_keyring() -> bool {
    std::env::var_os("PMS_NO_KEYRING").is_none()
}

fn entry() -> Result<keyring::Entry, keyring::Error> {
    keyring::Entry::new(KEYRING_SERVICE, KEYRING_USER)
}

pub fn load() -> Result<Option<StoredSession>> {
    if use_keyring() {
        if let Ok(text) = entry().and_then(|e| e.get_password()) {
            if let Ok(s) = serde_json::from_str(&text) {
                return Ok(Some(s));
            }
        }
    }
    let path = file_path()?;
    match fs::read_to_string(&path) {
        Ok(text) => {
            Ok(Some(serde_json::from_str(&text).with_context(|| {
                format!("bad session file {}", path.display())
            })?))
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e).with_context(|| format!("cannot read {}", path.display())),
    }
}

pub enum Saved {
    Keychain,
    File(PathBuf),
}

pub fn save(session: &StoredSession) -> Result<Saved> {
    let text = serde_json::to_string(session)?;
    if use_keyring() && entry().and_then(|e| e.set_password(&text)).is_ok() {
        // Do not keep an older copy in a file.
        if let Ok(path) = file_path() {
            let _ = fs::remove_file(path);
        }
        return Ok(Saved::Keychain);
    }
    let path = file_path()?;
    write_private(&path, text.as_bytes())?;
    Ok(Saved::File(path))
}

pub fn clear() -> Result<()> {
    if use_keyring() {
        if let Ok(e) = entry() {
            let _ = e.delete_credential();
        }
    }
    match fs::remove_file(file_path()?) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e.into()),
    }
}

/// Mode 0600 from the first byte, in a 0700 folder.
pub fn write_private(path: &std::path::Path, data: &[u8]) -> Result<()> {
    use std::io::Write;
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = fs::set_permissions(dir, fs::Permissions::from_mode(0o700));
        }
    }
    let mut opts = fs::OpenOptions::new();
    opts.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.mode(0o600);
    }
    let mut file = opts
        .open(path)
        .with_context(|| format!("cannot write {}", path.display()))?;
    file.write_all(data)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        // The file can exist with a wider mode from an older run.
        fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
    }
    Ok(())
}

pub struct SessionTokens {
    session: Mutex<StoredSession>,
}

impl SessionTokens {
    pub fn new(session: StoredSession) -> Self {
        Self {
            session: Mutex::new(session),
        }
    }

    async fn renew(session: &mut StoredSession) -> pms_client::Result<()> {
        // The session decides which Supabase project we talk to.
        let cfg = SupabaseConfig::for_session(&session.supabase_url)
            .map_err(|e| Error::Auth(e.to_string()))?;
        let resp = Supabase::new(cfg)
            .refresh(&session.refresh_token)
            .await
            .map_err(|e| Error::Auth(format!("Your session ended ({e}). Run `pms login`.")))?;
        *session = StoredSession::from_response(&resp, &session.supabase_url);
        if let Err(e) = save(session) {
            eprintln!("warning: cannot save the new session: {e:#}");
        }
        Ok(())
    }
}

#[async_trait]
impl TokenSource for SessionTokens {
    async fn access_token(&self) -> pms_client::Result<String> {
        let mut s = self.session.lock().await;
        if s.expires_at - now() < REFRESH_MARGIN {
            Self::renew(&mut s).await?;
        }
        Ok(s.access_token.clone())
    }

    async fn refresh(&self) -> pms_client::Result<String> {
        let mut s = self.session.lock().await;
        Self::renew(&mut s).await?;
        Ok(s.access_token.clone())
    }
}
