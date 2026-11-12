use clap::Parser;
use focusdeck::cli::{Cli, Command};
use focusdeck::config::Config;
use focusdeck::paths::Paths;
use focusdeck::profile;
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
        Some(Command::Capture { .. }) | Some(Command::Remind { .. }) => {
            eprintln!("henüz yok");
            Ok(ExitCode::from(2))
        }
        None => {
            let paths = Paths::from_env();
            let config = Config::load(&paths)?;
            let env_profile = std::env::var("FOCUSDECK_PROFILE").ok();
            let name =
                profile::select_name(cli.profile.as_deref(), env_profile.as_deref(), &config)?;
            let profile = profile::load(&paths.profile_path(&name))?;
            focusdeck::ui::run::tui(&paths, &config, profile).await?;
            Ok(ExitCode::SUCCESS)
        }
    }
}
