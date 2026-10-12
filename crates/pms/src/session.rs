use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{anyhow, Context, Result};
use async_trait::async_trait;
use pms_client::{Error, TokenSource};
use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;

use crate::config::config_dir;
use crate::supabase::{Supabase, SupabaseConfig, TokenResponse};

const FILE_NAME: &str = "credentials.json";
/// Refresh this long before the token ends, so that a request does not race the expiry.
const REFRESH_MARGIN: i64 = 60;

/// `PMS_TOKEN`, an API token. It wins over the saved session. Empty means unset.
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

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
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

/// The session file, `credentials.json` in the config folder.
fn file_path() -> Result<PathBuf> {
    Ok(config_dir()?.join(FILE_NAME))
}

/// The saved session, or `None` when there is no session file.
pub fn load() -> Result<Option<StoredSession>> {
    load_from(&file_path()?)
}

/// Saves the session. Returns the file that holds it.
pub fn save(session: &StoredSession) -> Result<PathBuf> {
    let path = file_path()?;
    save_to(&path, session)?;
    Ok(path)
}

/// Removes the session file. A missing file is not an error.
pub fn clear() -> Result<()> {
    clear_from(&file_path()?)
}

pub fn load_from(path: &Path) -> Result<Option<StoredSession>> {
    match fs::read_to_string(path) {
        Ok(text) => {
            let session = serde_json::from_str(&text)
                .with_context(|| format!("bad session file {}", path.display()))?;
            Ok(Some(session))
        }
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e).with_context(|| format!("cannot read {}", path.display())),
    }
}

pub fn save_to(path: &Path, session: &StoredSession) -> Result<()> {
    let text = serde_json::to_string(session)?;
    let dir = path
        .parent()
        .ok_or_else(|| anyhow!("bad session path {}", path.display()))?;
    if !dir.exists() {
        fs::create_dir_all(dir).with_context(|| format!("cannot create {}", dir.display()))?;
        // Only a folder that pms makes gets mode 0700. An existing folder keeps its mode.
        set_mode(dir, 0o700)?;
    }
    write_atomic(path, text.as_bytes())
}

pub fn clear_from(path: &Path) -> Result<()> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e).with_context(|| format!("cannot remove {}", path.display())),
    }
}

/// Writes a temp file in the same folder, then renames it over `path`.
/// The temp file is mode 0600 from its first byte.
fn write_atomic(path: &Path, data: &[u8]) -> Result<()> {
    let dir = path
        .parent()
        .ok_or_else(|| anyhow!("bad session path {}", path.display()))?;
    let name = path
        .file_name()
        .ok_or_else(|| anyhow!("bad session path {}", path.display()))?;
    let tmp = dir.join(format!(
        ".{}.{}.tmp",
        name.to_string_lossy(),
        std::process::id()
    ));
    let _ = fs::remove_file(&tmp);
    let result = write_then_rename(&tmp, path, data);
    if result.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    result.with_context(|| format!("cannot write {}", path.display()))
}

fn write_then_rename(tmp: &Path, path: &Path, data: &[u8]) -> io::Result<()> {
    let mut file = open_private(tmp)?;
    file.write_all(data)?;
    file.sync_all()?;
    fs::rename(tmp, path)
}

/// Creates a new file. On unix the mode is set at open, so no other user can read it.
fn open_private(path: &Path) -> io::Result<fs::File> {
    let mut opts = fs::OpenOptions::new();
    opts.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.mode(0o600);
    }
    opts.open(path)
}

#[cfg(unix)]
fn set_mode(path: &Path, mode: u32) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(mode))
        .with_context(|| format!("cannot set the mode of {}", path.display()))
}

#[cfg(not(unix))]
fn set_mode(_path: &Path, _mode: u32) -> Result<()> {
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

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(token: &str) -> StoredSession {
        StoredSession {
            access_token: token.to_owned(),
            refresh_token: "refresh-1".to_owned(),
            expires_at: 1_900_000_000,
            user_id: "user-1".to_owned(),
            email: "a@b.co".to_owned(),
            provider: "email".to_owned(),
            supabase_url: "https://example.supabase.co".to_owned(),
        }
    }

    /// A path in a folder that does not exist yet.
    fn new_path(dir: &tempfile::TempDir) -> PathBuf {
        dir.path().join("config").join(FILE_NAME)
    }

    fn file_names(dir: &Path) -> Vec<String> {
        let mut names: Vec<String> = fs::read_dir(dir)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        names
    }

    #[cfg(unix)]
    fn mode(path: &Path) -> u32 {
        use std::os::unix::fs::PermissionsExt;
        fs::metadata(path).unwrap().permissions().mode() & 0o777
    }

    #[test]
    #[cfg(unix)]
    fn new_file_and_folder_are_private() {
        let tmp = tempfile::TempDir::new().unwrap();
        let path = new_path(&tmp);
        save_to(&path, &sample("a")).unwrap();
        assert_eq!(mode(&path), 0o600);
        assert_eq!(mode(path.parent().unwrap()), 0o700);
    }

    #[test]
    #[cfg(unix)]
    fn existing_folder_keeps_its_mode() {
        let tmp = tempfile::TempDir::new().unwrap();
        let dir = tmp.path().join("config");
        fs::create_dir(&dir).unwrap();
        set_mode(&dir, 0o755).unwrap();
        save_to(&dir.join(FILE_NAME), &sample("a")).unwrap();
        assert_eq!(mode(&dir), 0o755);
    }

    #[test]
    #[cfg(unix)]
    fn save_replaces_a_wide_file_atomically() {
        let tmp = tempfile::TempDir::new().unwrap();
        let path = new_path(&tmp);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, "old text").unwrap();
        set_mode(&path, 0o644).unwrap();

        save_to(&path, &sample("new")).unwrap();

        // The new file has mode 0600 and the new content. No temp file is left.
        assert_eq!(mode(&path), 0o600);
        let text = fs::read_to_string(&path).unwrap();
        assert!(text.contains("\"access_token\":\"new\""));
        assert_eq!(file_names(path.parent().unwrap()), vec![FILE_NAME]);
    }

    #[test]
    fn save_load_clear_round_trip() {
        let tmp = tempfile::TempDir::new().unwrap();
        let path = new_path(&tmp);

        assert_eq!(load_from(&path).unwrap(), None);

        save_to(&path, &sample("a")).unwrap();
        assert_eq!(load_from(&path).unwrap(), Some(sample("a")));

        clear_from(&path).unwrap();
        assert!(!path.exists());
        assert_eq!(load_from(&path).unwrap(), None);
        // Clearing twice is fine.
        clear_from(&path).unwrap();
    }

    #[test]
    fn bad_session_file_is_an_error() {
        let tmp = tempfile::TempDir::new().unwrap();
        let path = new_path(&tmp);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, "not json").unwrap();
        assert!(load_from(&path).is_err());
    }
}
