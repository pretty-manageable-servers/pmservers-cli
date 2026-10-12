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

/// The result of a session lookup.
#[derive(Debug)]
pub enum Lookup {
    Session(StoredSession),
    /// No session exists.
    SignedOut,
    /// An old session is in the OS keychain, but pms cannot read it.
    Unreadable,
}

/// The session file, `credentials.json` in the config folder.
fn file_path() -> Result<PathBuf> {
    Ok(config_dir()?.join(FILE_NAME))
}

pub fn load() -> Result<Lookup> {
    load_from(&file_path()?, &OsKeychain)
}

/// Saves the session. Returns the file that holds it.
pub fn save(session: &StoredSession) -> Result<PathBuf> {
    let path = file_path()?;
    save_to(&path, session)?;
    Ok(path)
}

pub fn clear() -> Result<()> {
    clear_from(&file_path()?, &OsKeychain)
}

pub fn load_from(path: &Path, old: &dyn LegacyKeychain) -> Result<Lookup> {
    match fs::read_to_string(path) {
        Ok(text) => {
            let session = serde_json::from_str(&text)
                .with_context(|| format!("bad session file {}", path.display()))?;
            return Ok(Lookup::Session(session));
        }
        Err(e) if e.kind() == io::ErrorKind::NotFound => {}
        Err(e) => return Err(e).with_context(|| format!("cannot read {}", path.display())),
    }
    // ONE-TIME MOVE from the OS keychain. Remove this block, and LegacyKeychain, in a later release.
    // The keychain is read only when the session file is missing.
    match old.read() {
        LegacyRead::Found(text) => match serde_json::from_str::<StoredSession>(&text) {
            Ok(session) => {
                save_to(path, &session)?;
                old.delete();
                Ok(Lookup::Session(session))
            }
            Err(_) => Ok(Lookup::Unreadable),
        },
        LegacyRead::Empty => Ok(Lookup::SignedOut),
        LegacyRead::Failed => Ok(Lookup::Unreadable),
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

pub fn clear_from(path: &Path, old: &dyn LegacyKeychain) -> Result<()> {
    match fs::remove_file(path) {
        Ok(()) => {}
        Err(e) if e.kind() == io::ErrorKind::NotFound => {}
        Err(e) => return Err(e).with_context(|| format!("cannot remove {}", path.display())),
    }
    // Remove an old keychain item too, so that the one-time move cannot bring it back.
    old.delete();
    Ok(())
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

/// The old keychain item. Only the one-time move uses it. Remove it in a later release.
pub trait LegacyKeychain {
    fn read(&self) -> LegacyRead;
    /// Removes the item. It does not read the secret. Errors are ignored.
    fn delete(&self);
}

pub enum LegacyRead {
    Found(String),
    Empty,
    Failed,
}

/// The real keychain. Set `PMS_SKIP_KEYCHAIN_MOVE` in tests, so that they never read it.
struct OsKeychain;

const OLD_SERVICE: &str = "pms-cli";
const OLD_USER: &str = "session";

fn skip_keychain() -> bool {
    std::env::var_os("PMS_SKIP_KEYCHAIN_MOVE").is_some()
}

impl LegacyKeychain for OsKeychain {
    fn read(&self) -> LegacyRead {
        if skip_keychain() {
            return LegacyRead::Empty;
        }
        match keyring::Entry::new(OLD_SERVICE, OLD_USER).and_then(|e| e.get_password()) {
            Ok(text) => LegacyRead::Found(text),
            Err(keyring::Error::NoEntry) => LegacyRead::Empty,
            // On macOS, an item that exists but is denied is the only reason for another error.
            // On other systems, a missing keyring service means that there was no session.
            Err(_) if cfg!(target_os = "macos") => LegacyRead::Failed,
            Err(_) => LegacyRead::Empty,
        }
    }

    fn delete(&self) {
        if skip_keychain() {
            return;
        }
        // The keyring crate reads the secret before it deletes. On macOS, call the API that
        // only deletes, so that logout does not show a keychain prompt.
        #[cfg(target_os = "macos")]
        {
            let _ = security_framework::passwords::delete_generic_password(OLD_SERVICE, OLD_USER);
        }
        #[cfg(not(target_os = "macos"))]
        {
            if let Ok(entry) = keyring::Entry::new(OLD_SERVICE, OLD_USER) {
                let _ = entry.delete_credential();
            }
        }
    }
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
    use std::cell::Cell;

    use super::*;

    /// A keychain that does not touch the real one. It counts reads and deletes.
    struct FakeKeychain {
        text: Option<String>,
        fail: bool,
        reads: Cell<u32>,
        deletes: Cell<u32>,
    }

    impl FakeKeychain {
        fn new(text: Option<&str>, fail: bool) -> Self {
            Self {
                text: text.map(str::to_owned),
                fail,
                reads: Cell::new(0),
                deletes: Cell::new(0),
            }
        }
    }

    impl LegacyKeychain for FakeKeychain {
        fn read(&self) -> LegacyRead {
            self.reads.set(self.reads.get() + 1);
            if self.fail {
                return LegacyRead::Failed;
            }
            match &self.text {
                Some(t) => LegacyRead::Found(t.clone()),
                None => LegacyRead::Empty,
            }
        }

        fn delete(&self) {
            self.deletes.set(self.deletes.get() + 1);
        }
    }

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
        let keychain = FakeKeychain::new(None, false);

        assert!(matches!(
            load_from(&path, &keychain).unwrap(),
            Lookup::SignedOut
        ));

        save_to(&path, &sample("a")).unwrap();
        match load_from(&path, &keychain).unwrap() {
            Lookup::Session(s) => assert_eq!(s, sample("a")),
            other => panic!("expected a session, got {other:?}"),
        }

        clear_from(&path, &keychain).unwrap();
        assert!(!path.exists());
        assert!(matches!(
            load_from(&path, &keychain).unwrap(),
            Lookup::SignedOut
        ));
        // Clearing twice is fine.
        clear_from(&path, &keychain).unwrap();
    }

    #[test]
    fn file_present_means_no_keychain_read() {
        let tmp = tempfile::TempDir::new().unwrap();
        let path = new_path(&tmp);
        save_to(&path, &sample("file")).unwrap();
        let keychain =
            FakeKeychain::new(Some(&serde_json::to_string(&sample("kc")).unwrap()), false);

        match load_from(&path, &keychain).unwrap() {
            Lookup::Session(s) => assert_eq!(s.access_token, "file"),
            other => panic!("expected the file session, got {other:?}"),
        }
        assert_eq!(keychain.reads.get(), 0);
        assert_eq!(keychain.deletes.get(), 0);
    }

    #[test]
    fn move_from_keychain_writes_file_then_deletes_item() {
        let tmp = tempfile::TempDir::new().unwrap();
        let path = new_path(&tmp);
        let text = serde_json::to_string(&sample("kc")).unwrap();
        let keychain = FakeKeychain::new(Some(&text), false);

        match load_from(&path, &keychain).unwrap() {
            Lookup::Session(s) => assert_eq!(s, sample("kc")),
            other => panic!("expected the moved session, got {other:?}"),
        }
        assert_eq!(keychain.reads.get(), 1);
        assert_eq!(keychain.deletes.get(), 1);
        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            serde_json::to_string(&sample("kc")).unwrap()
        );

        // After the move, the keychain is not read again.
        load_from(&path, &keychain).unwrap();
        assert_eq!(keychain.reads.get(), 1);
    }

    #[test]
    fn denied_keychain_read_is_unreadable_and_writes_nothing() {
        let tmp = tempfile::TempDir::new().unwrap();
        let path = new_path(&tmp);
        let keychain = FakeKeychain::new(None, true);

        assert!(matches!(
            load_from(&path, &keychain).unwrap(),
            Lookup::Unreadable
        ));
        assert!(!path.exists());
        assert_eq!(keychain.reads.get(), 1);
        assert_eq!(keychain.deletes.get(), 0);
    }

    #[test]
    fn keychain_item_that_is_not_a_session_is_unreadable() {
        let tmp = tempfile::TempDir::new().unwrap();
        let path = new_path(&tmp);
        let keychain = FakeKeychain::new(Some("not json"), false);

        assert!(matches!(
            load_from(&path, &keychain).unwrap(),
            Lookup::Unreadable
        ));
        assert!(!path.exists());
        assert_eq!(keychain.deletes.get(), 0);
    }

    #[test]
    fn clear_removes_the_old_keychain_item() {
        let tmp = tempfile::TempDir::new().unwrap();
        let path = new_path(&tmp);
        let keychain = FakeKeychain::new(None, false);
        clear_from(&path, &keychain).unwrap();
        assert_eq!(keychain.deletes.get(), 1);
        assert_eq!(keychain.reads.get(), 0);
    }
}
