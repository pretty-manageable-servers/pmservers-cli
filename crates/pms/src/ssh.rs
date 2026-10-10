use std::path::Path;

use anyhow::{bail, Result};
use pms_client::vms::Vm;

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

/// `ssh` runs a second `ssh` as ProxyCommand to reach the jump host through cloudflared, and
/// forwards port 22 of the VM through it. This is the same path as `ssh -J pms-jump` with the
/// console's `Host pms-jump` block, but it needs no change in `~/.ssh/config`.
pub fn proxy_command(cloudflared: &str, jump_host: &str, identity: Option<&Path>) -> String {
    let via_cloudflared = format!(
        "ProxyCommand={} access ssh --hostname {}",
        shell_quote(cloudflared),
        shell_quote(jump_host)
    );
    let mut cmd = format!("ssh -o {}", shell_quote(&via_cloudflared));
    if let Some(id) = identity {
        cmd.push_str(&format!(
            " -i {} -o IdentitiesOnly=yes",
            shell_quote(&id.to_string_lossy())
        ));
    }
    cmd.push_str(&format!(" -W %h:%p -l jump {}", shell_quote(jump_host)));
    cmd
}

pub fn build_args(
    vm: &Vm,
    cloudflared: &str,
    identity: Option<&Path>,
    user: Option<&str>,
    extra: &[String],
) -> Result<Vec<String>> {
    if vm.status != "running" {
        bail!(
            "VM {} is {}. It must be running to connect.",
            vm.name,
            vm.status
        );
    }
    let Some(ip) = vm.private_ip.as_deref().filter(|ip| !ip.is_empty()) else {
        bail!(
            "VM {} has no private IP yet. Try again in a few seconds.",
            vm.name
        );
    };
    if vm.ssh_host.is_empty() {
        bail!("This API has no SSH jump host (ssh_host is empty), so there is no way in. This is normal for local development.");
    }
    let user = match user {
        Some(u) => u,
        None if !vm.ssh_user.is_empty() => &vm.ssh_user,
        None => bail!("The API did not say which user to use. Pass --user."),
    };
    let mut args = Vec::new();
    if let Some(id) = identity {
        args.push("-i".to_owned());
        args.push(id.to_string_lossy().into_owned());
        args.push("-o".to_owned());
        args.push("IdentitiesOnly=yes".to_owned());
    }
    // Private IPs are reused. The VM ID keeps one known_hosts entry for each VM.
    args.push("-o".to_owned());
    args.push(format!("HostKeyAlias={}", vm.id));
    args.push("-o".to_owned());
    args.push(format!(
        "ProxyCommand={}",
        proxy_command(cloudflared, &vm.ssh_host, identity)
    ));
    args.push(format!("{user}@{ip}"));
    args.extend(extra.iter().cloned());
    Ok(args)
}

/// Replace this process with `ssh`, so that the terminal and the exit code are ssh's own.
pub fn run(args: &[String]) -> Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        let err = std::process::Command::new("ssh").args(args).exec();
        if err.kind() == std::io::ErrorKind::NotFound {
            bail!("`ssh` was not found. Install an OpenSSH client.");
        }
        bail!("cannot run ssh: {err}");
    }
    #[cfg(not(unix))]
    {
        let status = std::process::Command::new("ssh").args(args).status()?;
        std::process::exit(status.code().unwrap_or(1));
    }
}

pub fn render_config(jump_host: &str, cloudflared: &str, vms: &[Vm]) -> String {
    let mut out = format!(
        "Host pms-jump\n  HostName {jump_host}\n  User jump\n  ProxyCommand {} access ssh --hostname %h\n",
        shell_quote(cloudflared)
    );
    for vm in vms {
        let Some(ip) = vm.private_ip.as_deref().filter(|ip| !ip.is_empty()) else {
            continue;
        };
        out.push_str(&format!(
            "\nHost pms-{}\n  HostName {ip}\n  User {}\n  ProxyJump pms-jump\n  HostKeyAlias {}\n",
            vm.name,
            if vm.ssh_user.is_empty() {
                "root"
            } else {
                &vm.ssh_user
            },
            vm.id
        ));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vm() -> Vm {
        serde_json::from_value(serde_json::json!({
            "id": "vm_abc", "name": "web", "status": "running", "size": "s", "image": "i",
            "disk_gb": 10, "private_ip": "10.0.0.5", "ssh_host": "jump.example.org",
            "ssh_user": "ubuntu", "created_at": "2026-10-09T10:00:00+00:00"
        }))
        .unwrap()
    }

    #[test]
    fn quoting() {
        assert_eq!(shell_quote("cloudflared"), "cloudflared");
        assert_eq!(shell_quote("/home/a b/cf"), "'/home/a b/cf'");
        assert_eq!(shell_quote("it's"), r"'it'\''s'");
    }

    #[test]
    fn proxy_command_reaches_the_jump_host_through_cloudflared() {
        assert_eq!(
            proxy_command("cloudflared", "jump.example.org", None),
            "ssh -o 'ProxyCommand=cloudflared access ssh --hostname jump.example.org' -W %h:%p -l jump jump.example.org"
        );
    }

    #[test]
    fn identity_is_used_for_the_jump_hop_too() {
        let cmd = proxy_command("cf", "j.example", Some(Path::new("/k/id")));
        assert!(cmd.contains("-i /k/id -o IdentitiesOnly=yes"));
    }

    #[test]
    fn builds_ssh_args() {
        let args = build_args(&vm(), "/opt/cf", None, None, &["uptime".into()]).unwrap();
        assert_eq!(args[0], "-o");
        assert_eq!(args[1], "HostKeyAlias=vm_abc");
        assert!(args[3].starts_with("ProxyCommand=ssh -o "));
        assert!(args[3].contains("/opt/cf access ssh --hostname jump.example.org"));
        assert_eq!(args[4], "ubuntu@10.0.0.5");
        assert_eq!(args[5], "uptime");
    }

    #[test]
    fn user_and_identity_overrides() {
        let args = build_args(&vm(), "cf", Some(Path::new("/k/id")), Some("root"), &[]).unwrap();
        assert_eq!(&args[..4], ["-i", "/k/id", "-o", "IdentitiesOnly=yes"]);
        assert_eq!(args.last().unwrap(), "root@10.0.0.5");
    }

    #[test]
    fn refuses_when_it_cannot_work() {
        let mut stopped = vm();
        stopped.status = "stopped".into();
        assert!(build_args(&stopped, "cf", None, None, &[])
            .unwrap_err()
            .to_string()
            .contains("stopped"));
        let mut no_ip = vm();
        no_ip.private_ip = None;
        assert!(build_args(&no_ip, "cf", None, None, &[])
            .unwrap_err()
            .to_string()
            .contains("no private IP"));
        let mut local = vm();
        local.ssh_host = String::new();
        assert!(build_args(&local, "cf", None, None, &[])
            .unwrap_err()
            .to_string()
            .contains("no SSH jump host"));
    }

    #[test]
    fn config_matches_the_console_block() {
        let text = render_config("jump.example.org", "cloudflared", &[vm()]);
        assert!(text.starts_with(
            "Host pms-jump\n  HostName jump.example.org\n  User jump\n  ProxyCommand cloudflared access ssh --hostname %h\n"
        ));
        assert!(text
            .contains("Host pms-web\n  HostName 10.0.0.5\n  User ubuntu\n  ProxyJump pms-jump\n"));
    }
}
