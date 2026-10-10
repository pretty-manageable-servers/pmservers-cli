use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use ssh_key::{rand_core::OsRng, Algorithm, LineEnding, PrivateKey};

use crate::session::write_private;

pub struct KeyPair {
    pub private_openssh: String,
    pub public_openssh: String,
}

/// An ed25519 key made on this machine. Only the public half is ever sent to the API.
pub fn generate(comment: &str) -> Result<KeyPair> {
    let mut key = PrivateKey::random(&mut OsRng, Algorithm::Ed25519)?;
    key.set_comment(comment);
    Ok(KeyPair {
        private_openssh: key.to_openssh(LineEnding::LF)?.to_string(),
        public_openssh: key.public_key().to_openssh()?,
    })
}

pub fn default_key_path(vm_name: &str) -> Result<PathBuf> {
    let home = dirs::home_dir().context("cannot find the home directory")?;
    Ok(home.join(".ssh").join(format!("pms_{vm_name}_ed25519")))
}

fn pub_path(private: &Path) -> PathBuf {
    let mut name = private.as_os_str().to_owned();
    name.push(".pub");
    PathBuf::from(name)
}

/// Fail before the VM is made if the key file would be overwritten.
pub fn check_free(private: &Path) -> Result<()> {
    for p in [private.to_path_buf(), pub_path(private)] {
        if p.exists() {
            bail!(
                "{} exists. Choose another file with --key-out.",
                p.display()
            );
        }
    }
    Ok(())
}

pub fn write_pair(private: &Path, pair: &KeyPair) -> Result<()> {
    check_free(private)?;
    write_private(private, pair.private_openssh.as_bytes())?;
    std::fs::write(pub_path(private), format!("{}\n", pair.public_openssh))
        .with_context(|| format!("cannot write {}", pub_path(private).display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generates_a_usable_ed25519_pair() {
        let pair = generate("pms-web").unwrap();
        assert!(pair.public_openssh.starts_with("ssh-ed25519 "));
        assert!(pair.public_openssh.ends_with(" pms-web"));
        assert!(pair
            .private_openssh
            .starts_with("-----BEGIN OPENSSH PRIVATE KEY-----"));
        // The public line is the half of the private key.
        let private = PrivateKey::from_openssh(&pair.private_openssh).unwrap();
        assert_eq!(
            private.public_key().to_openssh().unwrap(),
            pair.public_openssh
        );
    }

    #[test]
    fn two_keys_differ() {
        assert_ne!(
            generate("a").unwrap().public_openssh,
            generate("a").unwrap().public_openssh
        );
    }

    #[cfg(unix)]
    #[test]
    fn writes_private_key_with_mode_600_and_refuses_to_overwrite() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("sub").join("key");
        let pair = generate("c").unwrap();
        write_pair(&path, &pair).unwrap();
        assert_eq!(
            std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
        assert!(std::fs::read_to_string(pub_path(&path))
            .unwrap()
            .starts_with("ssh-ed25519 "));
        assert!(write_pair(&path, &pair).is_err());
    }
}
