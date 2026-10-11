//! Find `cloudflared`, or install a pinned copy for `pms db connect`.
//!
//! The version and the SHA-256 of each release asset are pinned here. Bump them on purpose.
//! The checksums were computed from the downloads of the GitHub release
//! (github.com/cloudflare/cloudflared/releases/tag/2026.10.0) and the linux-amd64 binary was run.
//! They were not compared with a checksum that Cloudflare publishes. A checksum is never taken
//! from the same download that it checks.
use std::io::Read;
use std::path::{Path, PathBuf};

use anyhow::{anyhow, bail, Context, Result};
use sha2::{Digest, Sha256};

use crate::config::data_dir;
use crate::ctx::Ctx;

pub const VERSION: &str = "2026.10.0";
const RELEASE_BASE: &str = "https://github.com/cloudflare/cloudflared/releases/download";

pub struct Asset {
    pub os: &'static str,
    pub arch: &'static str,
    pub name: &'static str,
    pub sha256: &'static str,
    /// The asset is a `.tgz` with one `cloudflared` file inside.
    pub archive: bool,
}

pub const ASSETS: [Asset; 4] = [
    Asset {
        os: "linux",
        arch: "x86_64",
        name: "cloudflared-linux-amd64",
        sha256: "d33ff2d14475178d2012c2c56beba87389ac5ded27649519f198a7d3134a99db",
        archive: false,
    },
    Asset {
        os: "linux",
        arch: "aarch64",
        name: "cloudflared-linux-arm64",
        sha256: "e6422b9d4f72d3194bc5a38676f13667c06666523217b842a877d72a80b5ac08",
        archive: false,
    },
    Asset {
        os: "macos",
        arch: "x86_64",
        name: "cloudflared-darwin-amd64.tgz",
        sha256: "903845b81828c8cb3c5d13d816a2de71c06a3da5785469df8eb0e1b736d92f9f",
        archive: true,
    },
    Asset {
        os: "macos",
        arch: "aarch64",
        name: "cloudflared-darwin-arm64.tgz",
        sha256: "a2f79ff7b9420aa537d74af239f376da170bbabeb529aec416002adac6a72e70",
        archive: true,
    },
];

pub fn asset_for(os: &str, arch: &str) -> Option<&'static Asset> {
    ASSETS.iter().find(|a| a.os == os && a.arch == arch)
}

/// `cloudflared` on `PATH`, if any.
pub fn on_path() -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .map(|dir| dir.join("cloudflared"))
        .find(|p| is_executable(p))
}

fn is_executable(p: &Path) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        p.metadata()
            .map(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
            .unwrap_or(false)
    }
    #[cfg(not(unix))]
    {
        p.is_file()
    }
}

/// The pinned version has its own file name, so a bump installs a new copy.
pub fn installed_path() -> Result<PathBuf> {
    Ok(data_dir()?
        .join("bin")
        .join(format!("cloudflared-{VERSION}")))
}

/// A copy on `PATH` wins. The installed copy is used only if there is none.
pub fn locate() -> Result<Option<PathBuf>> {
    if let Some(p) = on_path() {
        return Ok(Some(p));
    }
    let installed = installed_path()?;
    Ok(is_executable(&installed).then_some(installed))
}

pub async fn ensure(ctx: &Ctx) -> Result<PathBuf> {
    if let Some(p) = locate()? {
        return Ok(p);
    }
    let asset = asset_for(std::env::consts::OS, std::env::consts::ARCH).ok_or_else(|| {
        anyhow!(
            "cloudflared is needed for SSH and there is no download for {} {}. Install it yourself.",
            std::env::consts::OS,
            std::env::consts::ARCH
        )
    })?;
    let dest = installed_path()?;
    ctx.confirm(&format!(
        "SSH needs cloudflared, and it was not found. Download cloudflared {VERSION} from \
         github.com/cloudflare/cloudflared and install it in {}?",
        dest.parent().unwrap_or(&dest).display()
    ))
    .map_err(|e| anyhow!("{e} (or install cloudflared yourself and put it on PATH)"))?;
    install(&format!("{RELEASE_BASE}/{VERSION}"), asset, &dest).await?;
    eprintln!("Installed cloudflared {VERSION} at {}.", dest.display());
    Ok(dest)
}

fn is_sha256_hex(s: &str) -> bool {
    s.len() == 64 && s.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Refuse a missing, malformed or wrong checksum.
pub fn verify(data: &[u8], expected: &str) -> Result<()> {
    if !is_sha256_hex(expected) {
        bail!("There is no pinned checksum for this cloudflared download. Refusing to install it.");
    }
    let actual = hex(&Sha256::digest(data));
    if actual != expected {
        bail!("The cloudflared download does not match the pinned SHA-256 (got {actual}). Refusing to install it.");
    }
    Ok(())
}

fn extract_binary(archive: &[u8]) -> Result<Vec<u8>> {
    let mut tar = tar::Archive::new(flate2::read::GzDecoder::new(archive));
    for entry in tar.entries()? {
        let mut entry = entry?;
        let is_binary = entry
            .path()?
            .file_name()
            .is_some_and(|n| n == "cloudflared");
        if is_binary && entry.header().entry_type().is_file() {
            let mut out = Vec::new();
            entry.read_to_end(&mut out)?;
            return Ok(out);
        }
    }
    bail!("The cloudflared archive has no cloudflared file.")
}

pub async fn install(base_url: &str, asset: &Asset, dest: &Path) -> Result<()> {
    // A bad pin is found before the download starts.
    if !is_sha256_hex(asset.sha256) {
        bail!(
            "There is no pinned checksum for {}. Refusing to install it.",
            asset.name
        );
    }
    let url = format!("{base_url}/{}", asset.name);
    eprintln!("Downloading {url}");
    let data = reqwest::get(&url)
        .await
        .and_then(|r| r.error_for_status())
        .with_context(|| format!("cannot download {url}"))?
        .bytes()
        .await?;
    verify(&data, asset.sha256)?;
    let binary = if asset.archive {
        extract_binary(&data)?
    } else {
        data.to_vec()
    };
    write_executable(dest, &binary)
}

fn write_executable(dest: &Path, data: &[u8]) -> Result<()> {
    let dir = dest.parent().context("bad install path")?;
    std::fs::create_dir_all(dir)?;
    // Write beside the target, then rename: a half-written file is never found by `locate`.
    let tmp = dir.join(format!(".cloudflared-{}.tmp", std::process::id()));
    std::fs::write(&tmp, data)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o755))?;
    }
    std::fs::rename(&tmp, dest)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn sha(data: &[u8]) -> &'static str {
        Box::leak(hex(&Sha256::digest(data)).into_boxed_str())
    }

    fn tgz(content: &[u8]) -> Vec<u8> {
        let mut builder = tar::Builder::new(Vec::new());
        let mut header = tar::Header::new_gnu();
        header.set_size(content.len() as u64);
        header.set_mode(0o755);
        header.set_cksum();
        builder
            .append_data(&mut header, "cloudflared", content)
            .unwrap();
        let raw = builder.into_inner().unwrap();
        let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
        std::io::Write::write_all(&mut gz, &raw).unwrap();
        gz.finish().unwrap()
    }

    #[test]
    fn every_pinned_checksum_is_well_formed_and_assets_are_unique() {
        for a in &ASSETS {
            assert!(is_sha256_hex(a.sha256), "{}", a.name);
        }
        assert_eq!(
            asset_for("linux", "x86_64").unwrap().name,
            "cloudflared-linux-amd64"
        );
        assert_eq!(
            asset_for("macos", "aarch64").unwrap().name,
            "cloudflared-darwin-arm64.tgz"
        );
        assert!(asset_for("windows", "x86_64").is_none());
    }

    #[test]
    fn verify_refuses_placeholder_and_wrong_checksums() {
        assert!(verify(b"x", "").is_err());
        assert!(verify(b"x", "TODO").is_err());
        assert!(verify(b"x", &"0".repeat(64)).is_err());
        assert!(verify(b"x", sha(b"x")).is_ok());
    }

    #[tokio::test]
    async fn installs_a_raw_binary_after_the_check() {
        let server = MockServer::start().await;
        let body = b"#!/bin/sh\necho fake\n".to_vec();
        Mock::given(method("GET"))
            .and(path("/cloudflared-linux-amd64"))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(body.clone()))
            .mount(&server)
            .await;
        let asset = Asset {
            os: "linux",
            arch: "x86_64",
            name: "cloudflared-linux-amd64",
            sha256: sha(&body),
            archive: false,
        };
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("bin").join("cloudflared-test");
        install(&server.uri(), &asset, &dest).await.unwrap();
        assert_eq!(std::fs::read(&dest).unwrap(), body);
        assert!(is_executable(&dest));
    }

    #[tokio::test]
    async fn installs_from_a_tgz() {
        let server = MockServer::start().await;
        let archive = tgz(b"binary");
        Mock::given(method("GET"))
            .and(path("/cloudflared-darwin-arm64.tgz"))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(archive.clone()))
            .mount(&server)
            .await;
        let asset = Asset {
            os: "macos",
            arch: "aarch64",
            name: "cloudflared-darwin-arm64.tgz",
            sha256: sha(&archive),
            archive: true,
        };
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("cloudflared-test");
        install(&server.uri(), &asset, &dest).await.unwrap();
        assert_eq!(std::fs::read(&dest).unwrap(), b"binary");
    }

    #[tokio::test]
    async fn a_changed_download_is_refused_and_nothing_is_written() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/cloudflared-linux-amd64"))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(b"evil".to_vec()))
            .mount(&server)
            .await;
        let asset = Asset {
            os: "linux",
            arch: "x86_64",
            name: "cloudflared-linux-amd64",
            sha256: sha(b"good"),
            archive: false,
        };
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("cloudflared-test");
        let err = install(&server.uri(), &asset, &dest).await.unwrap_err();
        assert!(err.to_string().contains("does not match"));
        assert!(!dest.exists());
    }

    #[tokio::test]
    async fn a_placeholder_checksum_refuses_before_any_download() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(b"x".to_vec()))
            .expect(0)
            .mount(&server)
            .await;
        let asset = Asset {
            os: "linux",
            arch: "x86_64",
            name: "cloudflared-linux-amd64",
            sha256: "UNVERIFIED",
            archive: false,
        };
        let dir = tempfile::tempdir().unwrap();
        let err = install(&server.uri(), &asset, &dir.path().join("c"))
            .await
            .unwrap_err();
        assert!(err.to_string().contains("no pinned checksum"));
    }

    /// Run by hand when you bump the pin: `cargo test -p pms -- --ignored real_downloads`.
    #[tokio::test]
    #[ignore = "downloads about 120 MB from GitHub"]
    async fn real_downloads_match_the_pins() {
        for asset in &ASSETS {
            let dir = tempfile::tempdir().unwrap();
            let dest = dir.path().join("cloudflared");
            install(&format!("{RELEASE_BASE}/{VERSION}"), asset, &dest)
                .await
                .unwrap();
            assert!(is_executable(&dest), "{}", asset.name);
        }
    }
}
