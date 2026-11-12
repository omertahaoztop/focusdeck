use crate::config::Config;
use crate::install::{self, Index};
use crate::paths::Paths;
use crate::profile;
use clap::{Parser, Subcommand};
use std::io::Write;

#[derive(Debug, Parser)]
#[command(name = "focusdeck", version, about = "Odak panosu")]
pub struct Cli {
    /// Kullanılacak profil adı
    #[arg(long, global = true, value_name = "AD")]
    pub profile: Option<String>,
    #[command(subcommand)]
    pub command: Option<Command>,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Metni profildeki capture_into kutusuna ekler
    Capture {
        #[arg(value_name = "METİN")]
        text: Vec<String>,
    },
    /// Yaklaşan işleri listeler
    Remind {
        /// Bildirimi gönder
        #[arg(long)]
        send: bool,
    },
    /// Plugin kurar, listeler ve kaldırır
    Plugin {
        #[command(subcommand)]
        action: PluginCommand,
    },
}

#[derive(Debug, Subcommand)]
pub enum PluginCommand {
    /// Dizinden plugin kurar
    Install {
        #[arg(value_name = "AD")]
        name: String,
    },
    /// Kurulu plugin'leri listeler
    List,
    /// Plugin'i kaldırır
    Remove {
        #[arg(value_name = "AD")]
        name: String,
    },
}

pub async fn run_plugin(
    cmd: PluginCommand,
    paths: &Paths,
    config: &Config,
    out: &mut impl Write,
    err: &mut impl Write,
) -> anyhow::Result<()> {
    let plugin_dir = paths.plugin_dir();
    match cmd {
        PluginCommand::Install { name } => {
            let entry = install::install(&config.registries, &name, &plugin_dir).await?;
            writeln!(
                out,
                "{} {} kuruldu",
                display_safe(&name),
                display_safe(&entry.version)
            )?;
        }
        PluginCommand::List => list(&plugin_dir, config, out).await?,
        PluginCommand::Remove { name } => {
            install::validate_name(&name)?;
            for profile_name in profiles_using(paths, &name) {
                writeln!(
                    err,
                    "uyarı: {} profili bu plugin'i kullanıyor",
                    display_safe(&profile_name)
                )?;
            }
            install::remove(&plugin_dir, &name)?;
            writeln!(out, "{} kaldırıldı", display_safe(&name))?;
        }
    }
    Ok(())
}

async fn list(
    plugin_dir: &std::path::Path,
    config: &Config,
    out: &mut impl Write,
) -> anyhow::Result<()> {
    let installed = install::installed(plugin_dir);
    if installed.is_empty() {
        return Ok(());
    }
    let mut indexes: Vec<Index> = Vec::new();
    let mut unreachable = false;
    for registry in &config.registries {
        match install::load_index(registry).await {
            Ok(index) => indexes.push(index),
            Err(_) => unreachable = true,
        }
    }
    for (name, version) in installed {
        let latest = indexes
            .iter()
            .flat_map(|index| &index.plugin)
            .find(|entry| entry.name == name)
            .map(|entry| entry.version.as_str());
        let last = match latest {
            Some(latest) => format!("(dizinde: {})", display_safe(latest)),
            None if unreachable => "dizine ulaşılamadı".to_string(),
            None => "(dizinde: yok)".to_string(),
        };
        writeln!(
            out,
            "{}  {}  {last}",
            display_safe(&name),
            display_safe(&version)
        )?;
    }
    Ok(())
}

fn profiles_using(paths: &Paths, plugin: &str) -> Vec<String> {
    let Ok(dir) = std::fs::read_dir(paths.config_dir.join("profiles")) else {
        return Vec::new();
    };
    let mut files: Vec<_> = dir
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|ext| ext == "toml"))
        .collect();
    files.sort();
    files
        .iter()
        .filter_map(|path| profile::load(path).ok())
        .filter(|p| p.boxes.iter().any(|b| b.plugin == plugin))
        .map(|p| p.name)
        .collect()
}

/// Dizinden ya da profilden gelen metin terminal kaçış dizisi taşıyabilir.
fn display_safe(text: &str) -> String {
    text.chars()
        .map(|c| if c.is_control() { '?' } else { c })
        .collect()
}
