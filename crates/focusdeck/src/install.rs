use anyhow::{Context, bail};
use focusdeck_protocol::PROTOCOL_VERSION;
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
const USER_AGENT: &str = concat!("focusdeck/", env!("CARGO_PKG_VERSION"));

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

#[cfg(target_arch = "x86_64")]
pub fn current_target() -> &'static str {
    "x86_64-unknown-linux-gnu"
}

#[cfg(target_arch = "aarch64")]
pub fn current_target() -> &'static str {
    "aarch64-unknown-linux-gnu"
}

#[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
compile_error!("focusdeck yalnız x86_64 ve aarch64 Linux için derlenir");

/// Ad hem dosya yoluna hem manifest adına girdiği için düz dosya adı olmalı.
pub fn validate_name(name: &str) -> anyhow::Result<()> {
    if name.is_empty()
        || name.starts_with('.')
        || name.contains(['/', '\\', '\0'])
        || name.ends_with(".toml")
    {
        bail!("geçersiz plugin adı: {name:?}");
    }
    Ok(())
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
        bail!("sağlama tutmadı")
    }
}

pub async fn fetch(url: &str) -> anyhow::Result<Vec<u8>> {
    if let Some(path) = url.strip_prefix("file://") {
        if !path.starts_with('/') {
            bail!("file:// adresi mutlak yol olmalı: {url}");
        }
        return read_local(Path::new(path))
            .await
            .with_context(|| format!("{path} okunamadı"));
    }
    if url.starts_with("http://") || url.starts_with("https://") {
        return fetch_http(url).await;
    }
    bail!("desteklenmeyen adres (yalnız http, https, file): {url}")
}

async fn read_local(path: &Path) -> anyhow::Result<Vec<u8>> {
    use tokio::io::AsyncReadExt;
    let file = tokio::fs::File::open(path).await?;
    let mut buf = Vec::new();
    file.take(MAX_DOWNLOAD_BYTES as u64 + 1)
        .read_to_end(&mut buf)
        .await?;
    if buf.len() > MAX_DOWNLOAD_BYTES {
        bail!("dosya çok büyük");
    }
    Ok(buf)
}

async fn fetch_http(url: &str) -> anyhow::Result<Vec<u8>> {
    let no_downgrade = reqwest::redirect::Policy::custom(|attempt| {
        if attempt.previous().len() >= 5 {
            attempt.error("çok fazla yönlendirme")
        } else if attempt.url().scheme() != "https"
            && attempt.previous().iter().any(|u| u.scheme() == "https")
        {
            attempt.error("https adresinden http adresine yönlendirme reddedildi")
        } else {
            attempt.follow()
        }
    });
    let client = reqwest::Client::builder()
        .user_agent(USER_AGENT)
        .timeout(REQUEST_TIMEOUT)
        .connect_timeout(CONNECT_TIMEOUT)
        .redirect(no_downgrade)
        .build()
        .context("HTTP istemcisi kurulamadı")?;
    let mut response = client
        .get(url)
        .send()
        .await
        .with_context(|| format!("{url} indirilemedi"))?;
    let status = response.status();
    if !status.is_success() {
        bail!("{url} indirilemedi: HTTP {}", status.as_u16());
    }
    let mut buf = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .with_context(|| format!("{url} indirilemedi"))?
    {
        if buf.len() + chunk.len() > MAX_DOWNLOAD_BYTES {
            bail!("{url} çok büyük");
        }
        buf.extend_from_slice(&chunk);
    }
    Ok(buf)
}

pub async fn load_index(registry: &str) -> anyhow::Result<Index> {
    let bytes = fetch(registry).await?;
    let text = String::from_utf8(bytes).context("dizin UTF-8 değil")?;
    toml::from_str(&text).context("dizin geçersiz")
}

pub async fn install(
    registries: &[String],
    name: &str,
    plugin_dir: &Path,
) -> anyhow::Result<IndexEntry> {
    validate_name(name)?;
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
            bail!("dizinlerde böyle bir plugin yok: {name}");
        }
        bail!(
            "dizinlerde böyle bir plugin yok: {name} (ulaşılamayan dizinler: {})",
            failures.join("; ")
        );
    };
    if entry.protocol != PROTOCOL_VERSION {
        bail!(
            "{name} protokol {} istiyor, çekirdek {PROTOCOL_VERSION} biliyor",
            entry.protocol
        );
    }
    let target = entry.targets.get(current_target()).ok_or_else(|| {
        anyhow::anyhow!(
            "{name} için bu makineye uygun sürüm yok ({})",
            current_target()
        )
    })?;
    let bytes = fetch(&target.url).await?;
    if verify_sha256(&bytes, &target.sha256).is_err() {
        bail!("sağlama tutmadı: {name}");
    }
    let manifest = Manifest {
        version: entry.version.clone(),
        sha256: sha256_hex(&bytes),
        registry: registry.clone(),
    };
    std::fs::create_dir_all(plugin_dir)
        .with_context(|| format!("{} oluşturulamadı", plugin_dir.display()))?;
    write_binary(plugin_dir, name, &bytes)?;
    if let Err(e) = write_manifest(plugin_dir, name, &manifest) {
        let _ = std::fs::remove_file(plugin_dir.join(name));
        return Err(e);
    }
    Ok(entry)
}

fn write_binary(plugin_dir: &Path, name: &str, bytes: &[u8]) -> anyhow::Result<()> {
    let part = plugin_dir.join(format!(".{name}.part"));
    let result = write_part(&part, bytes).and_then(|()| {
        std::fs::rename(&part, plugin_dir.join(name)).context("plugin yerine konamadı")
    });
    if result.is_err() {
        let _ = std::fs::remove_file(&part);
    }
    result
}

fn write_part(part: &Path, bytes: &[u8]) -> anyhow::Result<()> {
    remove_if_exists(part)?;
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(part)
        .with_context(|| format!("{} oluşturulamadı", part.display()))?;
    file.write_all(bytes).context("plugin yazılamadı")?;
    file.sync_all().context("plugin diske işlenemedi")?;
    file.set_permissions(std::fs::Permissions::from_mode(0o755))
        .context("plugin çalıştırılabilir yapılamadı")
}

fn write_manifest(plugin_dir: &Path, name: &str, manifest: &Manifest) -> anyhow::Result<()> {
    let part = plugin_dir.join(format!(".{name}.toml.part"));
    let text = toml::to_string(manifest).context("manifest yazılamadı")?;
    let result = remove_if_exists(&part)
        .and_then(|()| {
            let mut file = std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o644)
                .open(&part)?;
            file.write_all(text.as_bytes())?;
            file.sync_all()?;
            Ok(())
        })
        .and_then(|()| {
            std::fs::rename(&part, plugin_dir.join(format!("{name}.toml")))
                .map_err(anyhow::Error::from)
        })
        .context("manifest yazılamadı");
    if result.is_err() {
        let _ = std::fs::remove_file(&part);
    }
    result
}

fn remove_if_exists(path: &Path) -> anyhow::Result<()> {
    match std::fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e).with_context(|| format!("{} silinemedi", path.display())),
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

pub fn remove(plugin_dir: &Path, name: &str) -> anyhow::Result<()> {
    validate_name(name)?;
    let mut removed = false;
    for file in [name.to_string(), format!("{name}.toml")] {
        let path = plugin_dir.join(file);
        match std::fs::remove_file(&path) {
            Ok(()) => removed = true,
            Err(e) if e.kind() == ErrorKind::NotFound => {}
            Err(e) => return Err(e).with_context(|| format!("{} silinemedi", path.display())),
        }
    }
    if !removed {
        bail!("plugin kurulu değil: {name}");
    }
    Ok(())
}
