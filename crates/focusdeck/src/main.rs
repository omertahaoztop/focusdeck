use clap::Parser;
use focusdeck::cli::{Cli, Command};
use focusdeck::config::Config;
use focusdeck::paths::Paths;
use focusdeck::profile::{self, Profile};
use focusdeck::{capture_cmd, remind};
use std::process::ExitCode;

#[tokio::main]
async fn main() -> ExitCode {
    let cli = Cli::parse();
    match run(cli).await {
        Ok(code) => code,
        Err(e) => {
            eprintln!("hata: {}", focusdeck::cli::render_error(&e));
            ExitCode::FAILURE
        }
    }
}

async fn run(cli: Cli) -> anyhow::Result<ExitCode> {
    match cli.command {
        Some(Command::Plugin { action }) => {
            let paths = Paths::from_env();
            let config = Config::load(&paths)?;
            focusdeck::cli::run_plugin(
                action,
                &paths,
                &config,
                &mut std::io::stdout(),
                &mut std::io::stderr(),
            )
            .await?;
            Ok(ExitCode::SUCCESS)
        }
        Some(Command::Capture { text }) => {
            let (paths, config, profile) = load_profile(cli.profile.as_deref())?;
            let text = (!text.is_empty()).then(|| text.join(" "));
            let now = chrono::Local::now().fixed_offset();
            let code = capture_cmd::run(&paths, &config, &profile, text, now).await?;
            Ok(ExitCode::from(u8::try_from(code).unwrap_or(1)))
        }
        Some(Command::Remind { send }) => {
            let (paths, _, profile) = load_profile(cli.profile.as_deref())?;
            let now = chrono::Local::now().fixed_offset();
            remind::run(&paths, &profile, send, now).await?;
            Ok(ExitCode::SUCCESS)
        }
        None => {
            let (paths, config, profile) = load_profile(cli.profile.as_deref())?;
            focusdeck::ui::run::tui(&paths, &config, profile).await?;
            Ok(ExitCode::SUCCESS)
        }
    }
}

fn load_profile(flag: Option<&str>) -> anyhow::Result<(Paths, Config, Profile)> {
    let paths = Paths::from_env();
    let config = Config::load(&paths)?;
    let env_profile = std::env::var("FOCUSDECK_PROFILE").ok();
    let name = profile::select_name(flag, env_profile.as_deref(), &config)?;
    let profile = profile::load(&paths.profile_path(&name))?;
    Ok((paths, config, profile))
}
