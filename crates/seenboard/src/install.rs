use crate::i18n::{msg, t, t_args};
use crate::local::LOCAL;
use anyhow::{Context, bail};
use seenboard_protocol::PROTOCOL_VERSION;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::io::{ErrorKind, Write};
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::time::Duration;

const MAX_DOWNLOAD_BYTES: usize = 256 * 1024 * 1024;
const REQUEST_TIMEOUT: Duration = Duration::from_secs(120);
const CONNECT_TIMEOUT: Duration = Duration::from_secs(15);
const USER_AGENT: &str = concat!("seenboard/", env!("CARGO_PKG_VERSION"));

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Index {
    #[serde(default)]
    pub plugin: Vec<IndexEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct IndexEntry {
    pub name: String,
    pub description: String,
    pub version: String,
    pub protocol: u32,
    #[serde(default)]
    pub targets: BTreeMap<String, Target>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Target {
    pub url: String,
    pub sha256: String,
}

#[derive(Serialize, Deserialize)]
struct Manifest {
    version: String,
    sha256: String,
    registry: String,
}

#[derive(Deserialize)]
struct Recorded {
    sha256: String,
}

#[cfg(target_arch = "x86_64")]
pub fn current_target() -> &'static str {
    "x86_64-unknown-linux-gnu"
}

#[cfg(target_arch = "aarch64")]
pub fn current_target() -> &'static str {
    "aarch64-unknown-linux-gnu"
}

#[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
compile_error!("seenboard builds only for x86_64 and aarch64 Linux");

/// Ad hem dosya yoluna hem manifest adına girdiği için düz dosya adı olmalı.
pub fn validate_name(name: &str) -> anyhow::Result<()> {
    if name.is_empty()
        || name.starts_with('.')
        || name.contains(['/', '\\', '\0'])
        || name.ends_with(".toml")
    {
        bail!(t_args(
            msg::INSTALL_INVALID_NAME,
            &[("name", format!("{name:?}").as_str().into())]
        ));
    }
    Ok(())
}

fn path_text(id: &str, path: &Path) -> String {
    t_args(id, &[("path", path.display().to_string().as_str().into())])
}

fn download_failed(url: &str) -> String {
    t_args(msg::INSTALL_DOWNLOAD_FAILED, &[("url", url.into())])
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

pub fn verify_sha256(bytes: &[u8], hex: &str) -> anyhow::Result<()> {
    if sha256_hex(bytes).eq_ignore_ascii_case(hex.trim()) {
        Ok(())
    } else {
        bail!(t(msg::INSTALL_CHECKSUM_MISMATCH))
    }
}

pub async fn fetch(url: &str) -> anyhow::Result<Vec<u8>> {
    if let Some(path) = url.strip_prefix("file://") {
        if !path.starts_with('/') {
            bail!(t_args(
                msg::INSTALL_FILE_URL_RELATIVE,
                &[("url", url.into())]
            ));
        }
        return read_local(Path::new(path))
            .await
            .with_context(|| t_args(msg::INSTALL_READ_FAILED, &[("path", path.into())]));
    }
    if url.starts_with("https://") {
        return fetch_http(url).await;
    }
    if url.starts_with("http://") {
        let loopback = reqwest::Url::parse(url).is_ok_and(|u| is_loopback(&u));
        if !loopback {
            bail!(insecure(url));
        }
        return fetch_http(url).await;
    }
    bail!(t_args(msg::INSTALL_UNSUPPORTED_URL, &[("url", url.into())]))
}

pub fn https_or_loopback(url: &str) -> bool {
    match reqwest::Url::parse(url) {
        Ok(u) => u.scheme() == "https" || (u.scheme() == "http" && is_loopback(&u)),
        Err(_) => false,
    }
}

fn is_loopback(url: &reqwest::Url) -> bool {
    matches!(url.host_str(), Some("127.0.0.1" | "[::1]" | "localhost"))
}

// an index from the network must not steer a download to a service on this machine
fn http_target_allowed(registry: &str, target: &str) -> bool {
    !target.starts_with("http://")
        || registry.starts_with("file://")
        || reqwest::Url::parse(registry).is_ok_and(|u| is_loopback(&u))
}

fn insecure(url: &str) -> String {
    t_args(msg::INSTALL_INSECURE_URL, &[("url", url.into())])
}

async fn read_local(path: &Path) -> anyhow::Result<Vec<u8>> {
    use tokio::io::AsyncReadExt;
    let file = tokio::fs::File::open(path).await?;
    let mut buf = Vec::new();
    file.take(MAX_DOWNLOAD_BYTES as u64 + 1)
        .read_to_end(&mut buf)
        .await?;
    if buf.len() > MAX_DOWNLOAD_BYTES {
        bail!(t(msg::INSTALL_FILE_TOO_LARGE));
    }
    Ok(buf)
}

// a loopback http hop is only for a user who pointed the installer at loopback themselves
fn redirect_allowed(first: &reqwest::Url, next: &reqwest::Url) -> bool {
    next.scheme() != "http" || (is_loopback(first) && is_loopback(next))
}

async fn fetch_http(url: &str) -> anyhow::Result<Vec<u8>> {
    let first = reqwest::Url::parse(url).with_context(|| download_failed(url))?;
    let https_only = reqwest::redirect::Policy::custom(move |attempt| {
        if attempt.previous().len() >= 5 {
            attempt.error(t(msg::INSTALL_TOO_MANY_REDIRECTS))
        } else if !redirect_allowed(&first, attempt.url()) {
            let message = insecure(attempt.url().as_str());
            attempt.error(message)
        } else {
            attempt.follow()
        }
    });
    let client = reqwest::Client::builder()
        .user_agent(USER_AGENT)
        .timeout(REQUEST_TIMEOUT)
        .connect_timeout(CONNECT_TIMEOUT)
        .redirect(https_only)
        .build()
        .with_context(|| t(msg::INSTALL_HTTP_CLIENT))?;
    let mut response = client
        .get(url)
        .send()
        .await
        .with_context(|| download_failed(url))?;
    let status = response.status();
    if !status.is_success() {
        bail!(t_args(
            msg::INSTALL_HTTP_STATUS,
            &[
                ("url", url.into()),
                ("status", status.as_u16().to_string().as_str().into())
            ]
        ));
    }
    let mut buf = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .with_context(|| download_failed(url))?
    {
        if buf.len() + chunk.len() > MAX_DOWNLOAD_BYTES {
            bail!(t_args(msg::INSTALL_URL_TOO_LARGE, &[("url", url.into())]));
        }
        buf.extend_from_slice(&chunk);
    }
    Ok(buf)
}

pub async fn load_index(registry: &str) -> anyhow::Result<Index> {
    let bytes = fetch(registry).await?;
    let text = String::from_utf8(bytes).with_context(|| t(msg::INSTALL_INDEX_NOT_UTF8))?;
    toml::from_str(&text).with_context(|| t(msg::INSTALL_INDEX_INVALID))
}

pub async fn install(
    registries: &[String],
    name: &str,
    plugin_dir: &Path,
) -> anyhow::Result<IndexEntry> {
    validate_name(name)?;
    if name == LOCAL {
        bail!(t(msg::INSTALL_LOCAL_BUILTIN));
    }
    let mut failures = Vec::new();
    let mut found = None;
    for registry in registries {
        match load_index(registry).await {
            Ok(index) => {
                if let Some(entry) = index.plugin.into_iter().find(|e| e.name == name) {
                    found = Some((registry, entry));
                    break;
                }
            }
            Err(e) => failures.push(format!("{registry}: {e:#}")),
        }
    }
    let Some((registry, entry)) = found else {
        if failures.is_empty() {
            bail!(t_args(msg::INSTALL_NOT_FOUND, &[("name", name.into())]));
        }
        bail!(t_args(
            msg::INSTALL_NOT_FOUND_UNREACHABLE,
            &[
                ("name", name.into()),
                ("failures", failures.join("; ").as_str().into())
            ]
        ));
    };
    if entry.protocol != PROTOCOL_VERSION {
        bail!(t_args(
            msg::INSTALL_PROTOCOL_MISMATCH,
            &[
                ("name", name.into()),
                ("wanted", entry.protocol.to_string().as_str().into()),
                ("known", PROTOCOL_VERSION.to_string().as_str().into())
            ]
        ));
    }
    let target = entry.targets.get(current_target()).ok_or_else(|| {
        anyhow::anyhow!(t_args(
            msg::INSTALL_NO_TARGET,
            &[("name", name.into()), ("target", current_target().into())]
        ))
    })?;
    if target.url.starts_with("file://") && !registry.starts_with("file://") {
        bail!(t_args(
            msg::INSTALL_REMOTE_FILE_TARGET,
            &[
                ("registry", registry.as_str().into()),
                ("name", name.into())
            ]
        ));
    }
    if !http_target_allowed(registry, &target.url) {
        bail!(insecure(&target.url));
    }
    let bytes = fetch(&target.url).await?;
    if verify_sha256(&bytes, &target.sha256).is_err() {
        bail!(t_args(
            msg::INSTALL_CHECKSUM_MISMATCH_NAMED,
            &[("name", name.into())]
        ));
    }
    let manifest = Manifest {
        version: entry.version.clone(),
        sha256: sha256_hex(&bytes),
        registry: registry.clone(),
    };
    std::fs::create_dir_all(plugin_dir)
        .with_context(|| path_text(msg::INSTALL_DIR_CREATE_FAILED, plugin_dir))?;
    publish(plugin_dir, name, &bytes, &manifest)?;
    Ok(entry)
}

// both files are staged before the first rename, and the previous binary stays linked until the manifest is in place
fn publish(plugin_dir: &Path, name: &str, bytes: &[u8], manifest: &Manifest) -> anyhow::Result<()> {
    let bin_part = plugin_dir.join(format!(".{name}.part"));
    let manifest_part = plugin_dir.join(format!(".{name}.toml.part"));
    let previous = plugin_dir.join(format!(".{name}.old"));
    let result = stage(&bin_part, &manifest_part, bytes, manifest)
        .and_then(|()| swap(plugin_dir, name, &bin_part, &manifest_part, &previous));
    for leftover in [&bin_part, &manifest_part, &previous] {
        let _ = std::fs::remove_file(leftover);
    }
    result
}

fn stage(
    bin_part: &Path,
    manifest_part: &Path,
    bytes: &[u8],
    manifest: &Manifest,
) -> anyhow::Result<()> {
    write_part(bin_part, bytes, 0o755)?;
    let text = toml::to_string(manifest).with_context(|| t(msg::INSTALL_MANIFEST_FAILED))?;
    write_part(manifest_part, text.as_bytes(), 0o644)
        .with_context(|| t(msg::INSTALL_MANIFEST_FAILED))
}

fn swap(
    plugin_dir: &Path,
    name: &str,
    bin_part: &Path,
    manifest_part: &Path,
    previous: &Path,
) -> anyhow::Result<()> {
    let target = plugin_dir.join(name);
    remove_if_exists(previous)?;
    let had_previous = match std::fs::hard_link(&target, previous) {
        Ok(()) => true,
        Err(e) if e.kind() == ErrorKind::NotFound => false,
        Err(e) => return Err(e).with_context(|| t(msg::INSTALL_REPLACE_FAILED)),
    };
    std::fs::rename(bin_part, &target).with_context(|| t(msg::INSTALL_REPLACE_FAILED))?;
    let manifest_path = plugin_dir.join(format!("{name}.toml"));
    if let Err(e) = std::fs::rename(manifest_part, manifest_path) {
        let _ = if had_previous {
            std::fs::rename(previous, &target)
        } else {
            std::fs::remove_file(&target)
        };
        return Err(e).with_context(|| t(msg::INSTALL_MANIFEST_FAILED));
    }
    Ok(())
}

fn write_part(part: &Path, bytes: &[u8], mode: u32) -> anyhow::Result<()> {
    remove_if_exists(part)?;
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(part)
        .with_context(|| path_text(msg::INSTALL_DIR_CREATE_FAILED, part))?;
    file.write_all(bytes)
        .with_context(|| t(msg::INSTALL_WRITE_FAILED))?;
    file.sync_all()
        .with_context(|| t(msg::INSTALL_SYNC_FAILED))?;
    file.set_permissions(std::fs::Permissions::from_mode(mode))
        .with_context(|| t(msg::INSTALL_CHMOD_FAILED))
}

fn remove_if_exists(path: &Path) -> anyhow::Result<()> {
    match std::fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e).with_context(|| path_text(msg::INSTALL_DELETE_FAILED, path)),
    }
}

pub fn verify_installed(plugin_dir: &Path, name: &str) -> anyhow::Result<()> {
    let changed = || {
        anyhow::anyhow!(t_args(
            msg::INSTALL_BINARY_CHANGED,
            &[("name", name.into())]
        ))
    };
    let text = match std::fs::read_to_string(plugin_dir.join(format!("{name}.toml"))) {
        Ok(text) => text,
        Err(e) if e.kind() == ErrorKind::NotFound => return Ok(()),
        Err(_) => return Err(changed()),
    };
    let recorded: Recorded = toml::from_str(&text).map_err(|_| changed())?;
    let mut file = std::fs::File::open(plugin_dir.join(name)).map_err(|_| changed())?;
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 64 * 1024];
    loop {
        match std::io::Read::read(&mut file, &mut buf) {
            Ok(0) => break,
            Ok(n) => hasher.update(&buf[..n]),
            Err(e) if e.kind() == ErrorKind::Interrupted => {}
            Err(_) => return Err(changed()),
        }
    }
    let actual: String = hasher
        .finalize()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    if actual.eq_ignore_ascii_case(recorded.sha256.trim()) {
        Ok(())
    } else {
        Err(changed())
    }
}

pub fn installed(plugin_dir: &Path) -> Vec<(String, String)> {
    let Ok(dir) = std::fs::read_dir(plugin_dir) else {
        return Vec::new();
    };
    let mut found: Vec<(String, String)> = dir
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let path: PathBuf = entry.path();
            let name = path.file_stem()?.to_str()?.to_string();
            if path.extension()? != "toml" || name.starts_with('.') {
                return None;
            }
            let text = std::fs::read_to_string(&path).ok()?;
            let manifest: toml::Table = toml::from_str(&text).ok()?;
            let version = manifest.get("version")?.as_str()?.to_string();
            Some((name, version))
        })
        .collect();
    found.sort();
    found
}

pub fn check_removable(name: &str) -> anyhow::Result<()> {
    validate_name(name)?;
    if name == LOCAL {
        bail!(t(msg::INSTALL_LOCAL_NOT_REMOVABLE));
    }
    Ok(())
}

pub fn remove(plugin_dir: &Path, name: &str) -> anyhow::Result<()> {
    check_removable(name)?;
    let mut removed = false;
    for file in [name.to_string(), format!("{name}.toml")] {
        let path = plugin_dir.join(file);
        match std::fs::remove_file(&path) {
            Ok(()) => removed = true,
            Err(e) if e.kind() == ErrorKind::NotFound => {}
            Err(e) => {
                return Err(e).with_context(|| path_text(msg::INSTALL_DELETE_FAILED, &path));
            }
        }
    }
    if !removed {
        bail!(t_args(msg::INSTALL_NOT_INSTALLED, &[("name", name.into())]));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_loopback_first_url_may_be_redirected_to_loopback_http() {
        let cases = [
            ("http://127.0.0.1:1/a", "http://localhost:2/b", true),
            ("https://registry.example/a", "http://127.0.0.1:1/b", false),
            (
                "https://registry.example/a",
                "https://other.example/b",
                true,
            ),
            (
                "https://registry.example/a",
                "http://other.example/b",
                false,
            ),
            ("http://127.0.0.1:1/a", "http://other.example/b", false),
            ("http://127.0.0.1:1/a", "https://other.example/b", true),
        ];
        for (first, next, allowed) in cases {
            let (first, next) = (
                reqwest::Url::parse(first).unwrap(),
                reqwest::Url::parse(next).unwrap(),
            );
            assert_eq!(
                redirect_allowed(&first, &next),
                allowed,
                "{first} -> {next}"
            );
        }
    }

    #[test]
    fn only_a_local_index_may_name_a_plain_http_download() {
        let cases = [
            (
                "https://registry.example/index.toml",
                "http://127.0.0.1:1/p",
                false,
            ),
            (
                "https://registry.example/index.toml",
                "http://other.example/p",
                false,
            ),
            (
                "https://registry.example/index.toml",
                "https://other.example/p",
                true,
            ),
            (
                "http://127.0.0.1:1/index.toml",
                "http://localhost:2/p",
                true,
            ),
            (
                "https://localhost:1/index.toml",
                "http://127.0.0.1:2/p",
                true,
            ),
            ("file:///srv/index.toml", "http://127.0.0.1:1/p", true),
        ];
        for (registry, target, allowed) in cases {
            assert_eq!(
                http_target_allowed(registry, target),
                allowed,
                "{registry} -> {target}"
            );
        }
    }
}
