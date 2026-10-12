use std::io::Read;
use std::path::Path;

use anyhow::{bail, Result};

/// Read a token from a private file (mode 0600 or stricter), or from standard input when `stdin` is set.
/// Returns None when neither is given. A token is never a command-line value.
pub fn read_token(file: Option<&Path>, stdin: bool) -> Result<Option<String>> {
    if let Some(path) = file {
        let metadata = std::fs::metadata(path)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            if metadata.permissions().mode() & 0o077 != 0 {
                bail!("{} must be private (chmod 600)", path.display());
            }
        }
        if !metadata.is_file() {
            bail!("{} is not a regular file", path.display());
        }
        let token = std::fs::read_to_string(path)?.trim().to_owned();
        return Ok(Some(token));
    }
    if stdin {
        let mut token = String::new();
        std::io::stdin().read_to_string(&mut token)?;
        return Ok(Some(token.trim().to_owned()));
    }
    Ok(None)
}
