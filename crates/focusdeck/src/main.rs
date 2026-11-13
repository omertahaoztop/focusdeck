use clap::FromArgMatches;
use focusdeck::cli::{Cli, Command};
use focusdeck::config::Config;
use focusdeck::i18n;
use focusdeck::paths::Paths;
use focusdeck::profile::{self, Profile};
use focusdeck::ui::run::Deck;
use focusdeck::{capture_cmd, remind};
use std::process::ExitCode;

fn main() -> ExitCode {
    if std::env::args_os()
        .nth(1)
        .is_some_and(|a| a == "serve-local")
    {
        let requested = focusdeck_i18n::requested(
            None,
            |var| std::env::var(var).ok(),
            &focusdeck_i18n::PLUGIN_ENV,
        );
        i18n::init(requested);
        return match focusdeck::local::plugin::serve_stdio() {
            Ok(()) => ExitCode::SUCCESS,
            Err(e) => {
                eprintln!("{} {e}", i18n::t(i18n::msg::ERROR_PREFIX));
                ExitCode::FAILURE
            }
        };
    }
    let runtime = match tokio::runtime::Runtime::new() {
        Ok(runtime) => runtime,
        Err(e) => {
            eprintln!("{} {e}", i18n::t(i18n::msg::ERROR_PREFIX));
            return ExitCode::FAILURE;
        }
    };
    runtime.block_on(async_main())
}

async fn async_main() -> ExitCode {
    init_language();
    let cli = Cli::from_arg_matches(&focusdeck::cli::command().get_matches())
        .unwrap_or_else(|e| e.exit());
    match run(cli).await {
        Ok(code) => code,
        Err(e) => {
            eprintln!(
                "{} {}",
                i18n::t(i18n::msg::ERROR_PREFIX),
                focusdeck::cli::render_error(&e)
            );
            ExitCode::FAILURE
        }
    }
}

fn init_language() {
    let language = Paths::from_env()
        .ok()
        .and_then(|paths| Config::load(&paths).ok())
        .and_then(|config| config.language);
    let requested = focusdeck_i18n::requested(
        language.as_deref(),
        |var| std::env::var(var).ok(),
        &focusdeck_i18n::CORE_ENV,
    );
    i18n::init(requested);
}

async fn run(cli: Cli) -> anyhow::Result<ExitCode> {
    match cli.command {
        Some(Command::Plugin { action }) => {
            let paths = Paths::from_env().map_err(anyhow::Error::msg)?;
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
        Some(Command::Server) => {
            let paths = Paths::from_env().map_err(anyhow::Error::msg)?;
            let config = Config::load(&paths)?;
            focusdeck::server::run(&paths, &config).await?;
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
            let paths = Paths::from_env().map_err(anyhow::Error::msg)?;
            let config = Config::load(&paths)?;
            let env_profile = std::env::var("FOCUSDECK_PROFILE").ok();
            let deck = match &config.server {
                Some(server) => {
                    let home = dirs::home_dir().unwrap_or_default();
                    let (client, name, view) = focusdeck::remote::open(
                        server,
                        &home,
                        cli.profile.as_deref(),
                        env_profile.as_deref(),
                        &config,
                    )
                    .await?;
                    Deck::Remote { client, name, view }
                }
                None => Deck::Local(local_profile(
                    &paths,
                    cli.profile.as_deref(),
                    env_profile.as_deref(),
                    &config,
                )?),
            };
            focusdeck::ui::run::tui(&paths, &config, deck).await?;
            Ok(ExitCode::SUCCESS)
        }
    }
}

fn load_profile(flag: Option<&str>) -> anyhow::Result<(Paths, Config, Profile)> {
    let paths = Paths::from_env().map_err(anyhow::Error::msg)?;
    let config = Config::load(&paths)?;
    let env_profile = std::env::var("FOCUSDECK_PROFILE").ok();
    let profile = local_profile(&paths, flag, env_profile.as_deref(), &config)?;
    Ok((paths, config, profile))
}

fn local_profile(
    paths: &Paths,
    flag: Option<&str>,
    env: Option<&str>,
    config: &Config,
) -> anyhow::Result<Profile> {
    let name = profile::resolve_name(paths, flag, env, config)?;
    Ok(profile::load(&paths.profile_path(&name))?)
}
