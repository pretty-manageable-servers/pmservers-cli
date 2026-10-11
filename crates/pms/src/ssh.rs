use std::path::Path;

use anyhow::{bail, Result};
use pms_client::databases::Database;

/// Single quotes for a shell. `ProxyCommand` is run by a shell.
pub fn shell_quote(s: &str) -> String {
    if !s.is_empty()
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"/._-=:@%+".contains(&b))
    {
        return s.to_owned();
    }
    format!("'{}'", s.replace('\'', r"'\''"))
}

/// `ssh -N -L` to the jump host. The jump host forwards the local port to port 5432 of the database.
/// The API puts the database address in the key line of the jump host, so no other address works.
pub fn database_args(
    db: &Database,
    cloudflared: &str,
    identity: Option<&Path>,
    port: u16,
) -> Result<Vec<String>> {
    if !matches!(db.status.as_str(), "running" | "read_only") {
        bail!(
            "Database {} is {}. It must be running to connect.",
            db.name,
            db.status
        );
    }
    let Some(ip) = db.private_ip.as_deref().filter(|ip| !ip.is_empty()) else {
        bail!(
            "Database {} has no private IP yet. Try again in a few seconds.",
            db.name
        );
    };
    if db.ssh_host.is_empty() {
        bail!("This API has no SSH jump host (ssh_host is empty), so there is no way in. This is normal for local development.");
    }
    let mut args = vec![
        "-N".to_owned(),
        "-o".to_owned(),
        "ExitOnForwardFailure=yes".to_owned(),
    ];
    // The tunnel stays open for a long time. Find a dead connection.
    args.extend(["-o".to_owned(), "ServerAliveInterval=30".to_owned()]);
    // Trust the jump host key on first use, so a run with no terminal works. A changed key is still refused.
    args.extend([
        "-o".to_owned(),
        "StrictHostKeyChecking=accept-new".to_owned(),
    ]);
    if let Some(id) = identity {
        args.push("-i".to_owned());
        args.push(id.to_string_lossy().into_owned());
        args.push("-o".to_owned());
        args.push("IdentitiesOnly=yes".to_owned());
    }
    args.push("-o".to_owned());
    args.push(format!(
        "ProxyCommand={} access ssh --hostname {}",
        shell_quote(cloudflared),
        shell_quote(&db.ssh_host)
    ));
    args.push("-L".to_owned());
    args.push(format!("127.0.0.1:{port}:{ip}:{}", db.port));
    args.push("-l".to_owned());
    args.push("jump".to_owned());
    args.push(db.ssh_host.clone());
    Ok(args)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quoting() {
        assert_eq!(shell_quote("cloudflared"), "cloudflared");
        assert_eq!(shell_quote("/home/a b/cf"), "'/home/a b/cf'");
        assert_eq!(shell_quote("it's"), r"'it'\''s'");
    }

    fn database() -> Database {
        serde_json::from_value(serde_json::json!({
            "id": "db_abc", "name": "main", "engine": "postgres", "size": "s", "vector": false,
            "status": "running", "host": "db.internal", "port": 5432, "database": "app",
            "user": "app", "private_ip": "10.43.0.9", "ssh_host": "jump.example.org",
            "storage_limit_bytes": 1, "created_at": "t", "updated_at": "t"
        }))
        .unwrap()
    }

    #[test]
    fn builds_database_forward_args() {
        let args = database_args(&database(), "/opt/cf", Some(Path::new("/k/id")), 6000).unwrap();
        assert_eq!(args[0], "-N");
        assert!(args.contains(&"ExitOnForwardFailure=yes".to_owned()));
        assert!(args.contains(&"StrictHostKeyChecking=accept-new".to_owned()));
        assert!(args.contains(&"IdentitiesOnly=yes".to_owned()));
        assert!(args
            .contains(&"ProxyCommand=/opt/cf access ssh --hostname jump.example.org".to_owned()));
        let l = args.iter().position(|a| a == "-L").unwrap();
        assert_eq!(args[l + 1], "127.0.0.1:6000:10.43.0.9:5432");
        assert_eq!(&args[args.len() - 3..], ["-l", "jump", "jump.example.org"]);
    }

    #[test]
    fn refuses_a_database_that_cannot_be_reached() {
        let check = |change: fn(&mut Database), text: &str| {
            let mut db = database();
            change(&mut db);
            let err = database_args(&db, "cf", None, 5432)
                .unwrap_err()
                .to_string();
            assert!(err.contains(text), "{err}");
        };
        check(|d| d.status = "provisioning".into(), "must be running");
        check(|d| d.private_ip = None, "no private IP");
        check(|d| d.private_ip = Some(String::new()), "no private IP");
        check(|d| d.ssh_host = String::new(), "no SSH jump host");
        let mut read_only = database();
        read_only.status = "read_only".into();
        assert!(database_args(&read_only, "cf", None, 5432).is_ok());
    }
}
