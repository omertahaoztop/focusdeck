use crate::config::Config;
use crate::i18n::{msg, t, t_args};
use crate::install::{self, Index};
use crate::local::LOCAL;
use crate::paths::Paths;
use crate::profile;
use clap::{Parser, Subcommand};
use std::io::Write;

#[derive(Debug, Parser)]
#[command(name = "focusdeck", version)]
pub struct Cli {
    #[arg(long, global = true)]
    pub profile: Option<String>,
    #[command(subcommand)]
    pub command: Option<Command>,
}

pub fn command() -> clap::Command {
    <Cli as clap::CommandFactory>::command()
        .about(t(msg::CLI_ABOUT))
        .mut_arg("profile", |a| {
            a.help(t(msg::CLI_PROFILE_HELP))
                .value_name(t(msg::CLI_NAME_VALUE))
        })
        .mut_subcommand("capture", |s| {
            s.about(t(msg::CLI_CAPTURE_ABOUT))
                .mut_arg("text", |a| a.value_name(t(msg::CLI_TEXT_VALUE)))
        })
        .mut_subcommand("remind", |s| {
            s.about(t(msg::CLI_REMIND_ABOUT))
                .mut_arg("send", |a| a.help(t(msg::CLI_REMIND_SEND_HELP)))
        })
        .mut_subcommand("plugin", |s| {
            s.about(t(msg::CLI_PLUGIN_ABOUT))
                .mut_subcommand("install", |s| {
                    s.about(t(msg::CLI_PLUGIN_INSTALL_ABOUT))
                        .mut_arg("name", |a| a.value_name(t(msg::CLI_NAME_VALUE)))
                })
                .mut_subcommand("list", |s| s.about(t(msg::CLI_PLUGIN_LIST_ABOUT)))
                .mut_subcommand("remove", |s| {
                    s.about(t(msg::CLI_PLUGIN_REMOVE_ABOUT))
                        .mut_arg("name", |a| a.value_name(t(msg::CLI_NAME_VALUE)))
                })
        })
}

#[derive(Debug, Subcommand)]
pub enum Command {
    Capture {
        text: Vec<String>,
    },
    Remind {
        #[arg(long)]
        send: bool,
    },
    Plugin {
        #[command(subcommand)]
        action: PluginCommand,
    },
}

#[derive(Debug, Subcommand)]
pub enum PluginCommand {
    Install { name: String },
    List,
    Remove { name: String },
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
            let line = t_args(
                msg::CLI_INSTALLED,
                &[
                    ("name", display_safe(&name).as_str().into()),
                    ("version", display_safe(&entry.version).as_str().into()),
                ],
            );
            writeln!(out, "{line}")?;
        }
        PluginCommand::List => list(&plugin_dir, config, out).await?,
        PluginCommand::Remove { name } => {
            install::check_removable(&name)?;
            for profile_name in profiles_using(paths, &name) {
                let line = t_args(
                    msg::CLI_IN_USE_WARNING,
                    &[("profile", display_safe(&profile_name).as_str().into())],
                );
                writeln!(err, "{line}")?;
            }
            install::remove(&plugin_dir, &name)?;
            let line = t_args(
                msg::CLI_REMOVED,
                &[("name", display_safe(&name).as_str().into())],
            );
            writeln!(out, "{line}")?;
        }
    }
    Ok(())
}

pub fn render_error(e: &anyhow::Error) -> String {
    display_safe(&format!("{e:#}").replace('\n', " "))
}

async fn list(
    plugin_dir: &std::path::Path,
    config: &Config,
    out: &mut impl Write,
) -> anyhow::Result<()> {
    let installed = install::installed(plugin_dir);
    let mut indexes: Vec<Index> = Vec::new();
    let mut unreachable = false;
    if !installed.is_empty() {
        for registry in &config.registries {
            match install::load_index(registry).await {
                Ok(index) => indexes.push(index),
                Err(_) => unreachable = true,
            }
        }
    }
    for (name, version) in installed {
        let latest = indexes
            .iter()
            .flat_map(|index| &index.plugin)
            .find(|entry| entry.name == name)
            .map(|entry| entry.version.as_str());
        let last = match latest {
            Some(latest) => t_args(
                msg::CLI_LATEST,
                &[("version", display_safe(latest).as_str().into())],
            ),
            None if unreachable => t(msg::CLI_REGISTRY_UNREACHABLE),
            None => t(msg::CLI_NOT_IN_REGISTRY),
        };
        writeln!(
            out,
            "{}  {}  {last}",
            display_safe(&name),
            display_safe(&version)
        )?;
    }
    writeln!(out, "{LOCAL}  {}", t(msg::CLI_BUILTIN))?;
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
pub fn display_safe(text: &str) -> String {
    text.chars()
        .map(|c| if c.is_control() { '?' } else { c })
        .collect()
}
