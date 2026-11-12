use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use clap::Parser;
use focusdeck::cli::{Cli, Command, PluginCommand, render_error, run_plugin};
use focusdeck::config::Config;
use focusdeck::install::{self, current_target, sha256_hex, verify_sha256};
use focusdeck::paths::Paths;

const BIN: &[u8] = b"#!/bin/sh\nexit 0\n";

fn write_registry(dir: &Path, name: &str, version: &str, protocol: u32, sha: &str, target: &str) {
    std::fs::write(dir.join(name), BIN).unwrap();
    let index = format!(
        "[[plugin]]\nname = \"{name}\"\ndescription = \"deneme\"\nversion = \"{version}\"\nprotocol = {protocol}\n\n[plugin.targets.{target}]\nurl = \"file://{}\"\nsha256 = \"{sha}\"\n",
        dir.join(name).display()
    );
    std::fs::write(dir.join("index.toml"), index).unwrap();
}

fn reg_url(dir: &Path) -> String {
    format!("file://{}", dir.join("index.toml").display())
}

fn good_registry(dir: &Path) -> String {
    write_registry(
        dir,
        "redmine",
        "0.1.0",
        1,
        &sha256_hex(BIN),
        current_target(),
    );
    reg_url(dir)
}

fn entries(dir: &Path) -> Vec<String> {
    let mut v: Vec<String> = std::fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    v.sort();
    v
}

#[tokio::test]
async fn installs_and_verifies() {
    let reg = tempfile::tempdir().unwrap();
    let plugins = tempfile::tempdir().unwrap();
    let url = good_registry(reg.path());
    let entry = install::install(std::slice::from_ref(&url), "redmine", plugins.path())
        .await
        .unwrap();
    assert_eq!(entry.version, "0.1.0");
    let bin = plugins.path().join("redmine");
    assert_eq!(std::fs::read(&bin).unwrap(), BIN);
    assert_eq!(
        std::fs::metadata(&bin).unwrap().permissions().mode() & 0o777,
        0o755
    );
    let manifest: toml::Table =
        toml::from_str(&std::fs::read_to_string(plugins.path().join("redmine.toml")).unwrap())
            .unwrap();
    assert_eq!(manifest["version"].as_str(), Some("0.1.0"));
    assert_eq!(manifest["sha256"].as_str(), Some(sha256_hex(BIN).as_str()));
    assert_eq!(manifest["registry"].as_str(), Some(url.as_str()));
    assert_eq!(entries(plugins.path()), ["redmine", "redmine.toml"]);
}

#[tokio::test]
async fn uppercase_index_sha_is_accepted() {
    let reg = tempfile::tempdir().unwrap();
    let plugins = tempfile::tempdir().unwrap();
    write_registry(
        reg.path(),
        "redmine",
        "0.1.0",
        1,
        &sha256_hex(BIN).to_uppercase(),
        current_target(),
    );
    install::install(&[reg_url(reg.path())], "redmine", plugins.path())
        .await
        .unwrap();
    let manifest = std::fs::read_to_string(plugins.path().join("redmine.toml")).unwrap();
    assert!(manifest.contains(&sha256_hex(BIN)));
}

#[tokio::test]
async fn creates_missing_plugin_dir() {
    let reg = tempfile::tempdir().unwrap();
    let base = tempfile::tempdir().unwrap();
    let plugins = base.path().join("a/b/plugins");
    install::install(&[good_registry(reg.path())], "redmine", &plugins)
        .await
        .unwrap();
    assert!(plugins.join("redmine").is_file());
}

#[tokio::test]
async fn checksum_mismatch_writes_nothing() {
    let reg = tempfile::tempdir().unwrap();
    let plugins = tempfile::tempdir().unwrap();
    write_registry(
        reg.path(),
        "redmine",
        "0.1.0",
        1,
        &"0".repeat(64),
        current_target(),
    );
    let err = install::install(&[reg_url(reg.path())], "redmine", plugins.path())
        .await
        .unwrap_err();
    assert_eq!(err.to_string(), "checksum mismatch: redmine");
    assert!(entries(plugins.path()).is_empty());
}

#[tokio::test]
async fn failed_reinstall_keeps_previous_version() {
    let reg = tempfile::tempdir().unwrap();
    let plugins = tempfile::tempdir().unwrap();
    install::install(&[good_registry(reg.path())], "redmine", plugins.path())
        .await
        .unwrap();
    write_registry(
        reg.path(),
        "redmine",
        "0.2.0",
        1,
        &"0".repeat(64),
        current_target(),
    );
    install::install(&[reg_url(reg.path())], "redmine", plugins.path())
        .await
        .unwrap_err();
    assert_eq!(std::fs::read(plugins.path().join("redmine")).unwrap(), BIN);
    assert!(
        std::fs::read_to_string(plugins.path().join("redmine.toml"))
            .unwrap()
            .contains("0.1.0")
    );
    assert_eq!(entries(plugins.path()), ["redmine", "redmine.toml"]);
}

#[tokio::test]
async fn stale_part_file_is_replaced() {
    let reg = tempfile::tempdir().unwrap();
    let plugins = tempfile::tempdir().unwrap();
    std::fs::write(plugins.path().join(".redmine.part"), b"eski").unwrap();
    install::install(&[good_registry(reg.path())], "redmine", plugins.path())
        .await
        .unwrap();
    assert_eq!(entries(plugins.path()), ["redmine", "redmine.toml"]);
}

#[tokio::test]
async fn missing_target_writes_nothing() {
    let reg = tempfile::tempdir().unwrap();
    let plugins = tempfile::tempdir().unwrap();
    write_registry(
        reg.path(),
        "redmine",
        "0.1.0",
        1,
        &sha256_hex(BIN),
        "riscv64gc-unknown-linux-gnu",
    );
    let err = install::install(&[reg_url(reg.path())], "redmine", plugins.path())
        .await
        .unwrap_err();
    assert_eq!(
        err.to_string(),
        format!(
            "no build of redmine for this machine ({})",
            current_target()
        )
    );
    assert!(entries(plugins.path()).is_empty());
}

#[tokio::test]
async fn first_registry_wins() {
    let a = tempfile::tempdir().unwrap();
    let b = tempfile::tempdir().unwrap();
    let plugins = tempfile::tempdir().unwrap();
    let bin_a = b"#!/bin/sh\necho a\n";
    let bin_b = b"#!/bin/sh\necho b\n";
    for (dir, bin) in [(a.path(), &bin_a[..]), (b.path(), &bin_b[..])] {
        std::fs::write(dir.join("redmine"), bin).unwrap();
        std::fs::write(
            dir.join("index.toml"),
            format!(
                "[[plugin]]\nname = \"redmine\"\ndescription = \"d\"\nversion = \"0.1.0\"\nprotocol = 1\n[plugin.targets.{}]\nurl = \"file://{}\"\nsha256 = \"{}\"\n",
                current_target(),
                dir.join("redmine").display(),
                sha256_hex(bin)
            ),
        )
        .unwrap();
    }
    install::install(
        &[reg_url(a.path()), reg_url(b.path())],
        "redmine",
        plugins.path(),
    )
    .await
    .unwrap();
    assert_eq!(
        std::fs::read(plugins.path().join("redmine")).unwrap(),
        bin_a
    );
}

#[tokio::test]
async fn unreachable_registry_is_skipped() {
    let reg = tempfile::tempdir().unwrap();
    let plugins = tempfile::tempdir().unwrap();
    let missing = reg_url(&reg.path().join("yok"));
    let good = tempfile::tempdir().unwrap();
    install::install(
        &[missing, good_registry(good.path())],
        "redmine",
        plugins.path(),
    )
    .await
    .unwrap();
    assert!(plugins.path().join("redmine").is_file());
}

#[tokio::test]
async fn unknown_plugin_error() {
    let reg = tempfile::tempdir().unwrap();
    let plugins = tempfile::tempdir().unwrap();
    let url = good_registry(reg.path());
    let err = install::install(&[url], "yok", plugins.path())
        .await
        .unwrap_err();
    assert_eq!(err.to_string(), "no such plugin in the registries: yok");
}

#[tokio::test]
async fn protocol_mismatch_rejected() {
    let reg = tempfile::tempdir().unwrap();
    let plugins = tempfile::tempdir().unwrap();
    write_registry(
        reg.path(),
        "redmine",
        "0.1.0",
        2,
        &sha256_hex(BIN),
        current_target(),
    );
    let err = install::install(&[reg_url(reg.path())], "redmine", plugins.path())
        .await
        .unwrap_err();
    assert_eq!(
        err.to_string(),
        "redmine wants protocol 2, the core knows 1"
    );
    assert!(entries(plugins.path()).is_empty());
}

#[tokio::test]
async fn unsafe_names_are_rejected_before_any_io() {
    let plugins = tempfile::tempdir().unwrap();
    let plugin_dir: PathBuf = plugins.path().join("plugins");
    for bad in [
        "", ".", "..", "../x", "a/b", "a\\b", ".gizli", "a\0b", "x.toml",
    ] {
        let err = install::install(&["file:///yok/index.toml".to_string()], bad, &plugin_dir)
            .await
            .unwrap_err();
        assert!(err.to_string().contains("invalid plugin name"), "{bad:?}");
        assert!(install::remove(&plugin_dir, bad).is_err(), "{bad:?}");
    }
    assert!(!plugin_dir.exists());
}

#[tokio::test]
async fn local_is_reserved_before_any_io() {
    let mut server = mockito::Server::new_async().await;
    let index = server
        .mock("GET", "/index.toml")
        .expect(0)
        .create_async()
        .await;
    let plugins = tempfile::tempdir().unwrap();
    let plugin_dir: PathBuf = plugins.path().join("plugins");
    let err = install::install(
        &[format!("{}/index.toml", server.url())],
        "local",
        &plugin_dir,
    )
    .await
    .unwrap_err();
    assert_eq!(
        err.to_string(),
        "local comes with focusdeck, there is nothing to install"
    );
    let err = install::remove(&plugin_dir, "local").unwrap_err();
    assert_eq!(
        err.to_string(),
        "local comes with focusdeck and cannot be removed"
    );
    assert!(!plugin_dir.exists());
    index.assert_async().await;
}

#[test]
fn plugin_install_and_remove_refuse_local() {
    let dir = tempfile::tempdir().unwrap();
    let paths = test_paths(dir.path());
    write_profile(&paths, "ev.toml", "ev", "local");
    let (result, out, err) = run(
        PluginCommand::Install {
            name: "local".into(),
        },
        &paths,
        vec!["http://127.0.0.1:1/index.toml".into()],
    );
    assert!(
        result
            .unwrap_err()
            .to_string()
            .contains("comes with focusdeck")
    );
    assert!(out.is_empty() && err.is_empty());
    let (result, out, err) = run(
        PluginCommand::Remove {
            name: "local".into(),
        },
        &paths,
        vec![],
    );
    assert!(
        result
            .unwrap_err()
            .to_string()
            .contains("comes with focusdeck")
    );
    assert!(out.is_empty() && err.is_empty(), "{out}{err}");
    assert!(!paths.plugin_dir().exists());
}

#[test]
fn plugin_list_puts_local_after_installed_plugins() {
    let dir = tempfile::tempdir().unwrap();
    let paths = test_paths(dir.path());
    let plugins = paths.plugin_dir();
    std::fs::create_dir_all(&plugins).unwrap();
    std::fs::write(plugins.join("vikunja.toml"), "version = \"0.2.0\"\n").unwrap();
    let (result, out, _) = run(PluginCommand::List, &paths, vec![]);
    result.unwrap();
    assert_eq!(
        out,
        "vikunja  0.2.0  (in registry: none)\nlocal  (built-in)\n"
    );
}

#[tokio::test]
async fn index_entry_url_with_unsupported_scheme_is_error() {
    let reg = tempfile::tempdir().unwrap();
    let plugins = tempfile::tempdir().unwrap();
    for url in ["ftp://x/y", "relative/path", "file://relative/path"] {
        std::fs::write(
            reg.path().join("index.toml"),
            format!(
                "[[plugin]]\nname = \"redmine\"\ndescription = \"d\"\nversion = \"0.1.0\"\nprotocol = 1\n[plugin.targets.{}]\nurl = \"{url}\"\nsha256 = \"{}\"\n",
                current_target(),
                sha256_hex(BIN)
            ),
        )
        .unwrap();
        install::install(&[reg_url(reg.path())], "redmine", plugins.path())
            .await
            .unwrap_err();
        assert!(entries(plugins.path()).is_empty(), "{url}");
    }
}

#[tokio::test]
async fn http_fetch_sends_user_agent_and_fails_on_non_2xx() {
    let mut server = mockito::Server::new_async().await;
    let ok = server
        .mock("GET", "/ok")
        .match_header(
            "user-agent",
            concat!("focusdeck/", env!("CARGO_PKG_VERSION")),
        )
        .with_body("içerik")
        .create_async()
        .await;
    let missing = server
        .mock("GET", "/yok")
        .with_status(404)
        .with_body("gizli gövde")
        .create_async()
        .await;
    let bytes = install::fetch(&format!("{}/ok", server.url()))
        .await
        .unwrap();
    assert_eq!(bytes, "içerik".as_bytes());
    let err = install::fetch(&format!("{}/yok", server.url()))
        .await
        .unwrap_err();
    let text = format!("{err:#}");
    assert!(text.contains("404"), "{text}");
    assert!(!text.contains("gizli gövde"));
    ok.assert_async().await;
    missing.assert_async().await;
}

#[test]
fn sha256_verification() {
    let hex = sha256_hex(BIN);
    assert_eq!(hex.len(), 64);
    verify_sha256(BIN, &hex).unwrap();
    verify_sha256(BIN, &hex.to_uppercase()).unwrap();
    assert!(verify_sha256(BIN, &"0".repeat(64)).is_err());
    assert!(verify_sha256(b"baska", &hex).is_err());
    assert!(verify_sha256(BIN, "").is_err());
}

#[test]
fn remove_deletes_binary_and_manifest() {
    let plugins = tempfile::tempdir().unwrap();
    std::fs::write(plugins.path().join("redmine"), BIN).unwrap();
    std::fs::write(plugins.path().join("redmine.toml"), "version = \"0.1.0\"\n").unwrap();
    std::fs::write(plugins.path().join("diger"), BIN).unwrap();
    install::remove(plugins.path(), "redmine").unwrap();
    assert_eq!(entries(plugins.path()), ["diger"]);
    let err = install::remove(plugins.path(), "redmine").unwrap_err();
    assert_eq!(err.to_string(), "plugin is not installed: redmine");
}

#[test]
fn installed_lists_name_and_version() {
    let plugins = tempfile::tempdir().unwrap();
    for (name, version) in [("zulip", "0.3.0"), ("redmine", "0.1.0")] {
        std::fs::write(plugins.path().join(name), BIN).unwrap();
        std::fs::write(
            plugins.path().join(format!("{name}.toml")),
            format!("version = \"{version}\"\nsha256 = \"x\"\nregistry = \"r\"\n"),
        )
        .unwrap();
    }
    std::fs::write(plugins.path().join(".gizli.part"), b"x").unwrap();
    assert_eq!(
        install::installed(plugins.path()),
        [
            ("redmine".to_string(), "0.1.0".to_string()),
            ("zulip".to_string(), "0.3.0".to_string())
        ]
    );
    assert!(install::installed(&plugins.path().join("yok")).is_empty());
}

fn test_paths(dir: &Path) -> Paths {
    Paths {
        config_dir: dir.join("config"),
        data_dir: dir.join("data"),
        state_dir: dir.join("state"),
    }
}

fn run(
    cmd: PluginCommand,
    paths: &Paths,
    registries: Vec<String>,
) -> (anyhow::Result<()>, String, String) {
    let config = Config {
        registries,
        ..Config::default()
    };
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let result = rt.block_on(run_plugin(cmd, paths, &config, &mut out, &mut err));
    (
        result,
        String::from_utf8(out).unwrap(),
        String::from_utf8(err).unwrap(),
    )
}

fn write_profile(paths: &Paths, file: &str, name: &str, plugin: &str) {
    let dir = paths.config_dir.join("profiles");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join(file),
        format!(
            "name = \"{name}\"\n[[box]]\nid = \"a\"\ntitle = \"A\"\nplugin = \"{plugin}\"\nrow = 0\ncol = 0\n"
        ),
    )
    .unwrap();
}

#[test]
fn plugin_list_is_empty_without_network() {
    let dir = tempfile::tempdir().unwrap();
    let paths = test_paths(dir.path());
    let (result, out, err) = run(
        PluginCommand::List,
        &paths,
        vec!["http://127.0.0.1:1/index.toml".into()],
    );
    result.unwrap();
    assert_eq!(out, "local  (built-in)\n");
    assert!(err.is_empty());
}

#[test]
fn plugin_install_list_remove_flow() {
    let dir = tempfile::tempdir().unwrap();
    let reg = tempfile::tempdir().unwrap();
    let paths = test_paths(dir.path());
    let regs = vec![good_registry(reg.path())];

    let (result, out, _) = run(
        PluginCommand::Install {
            name: "redmine".into(),
        },
        &paths,
        regs.clone(),
    );
    result.unwrap();
    assert!(out.contains("redmine") && out.contains("0.1.0"), "{out}");

    let (result, out, _) = run(PluginCommand::List, &paths, regs.clone());
    result.unwrap();
    assert_eq!(
        out,
        "redmine  0.1.0  (in registry: 0.1.0)\nlocal  (built-in)\n"
    );

    let (result, out, _) = run(
        PluginCommand::List,
        &paths,
        vec![reg_url(&reg.path().join("yok"))],
    );
    result.unwrap();
    assert_eq!(
        out,
        "redmine  0.1.0  registry unreachable\nlocal  (built-in)\n"
    );

    write_profile(&paths, "is.toml", "iş", "redmine");
    write_profile(&paths, "ev.toml", "ev", "vikunja");
    std::fs::write(paths.config_dir.join("profiles/bozuk.toml"), "name = [").unwrap();
    let (result, _, err) = run(
        PluginCommand::Remove {
            name: "redmine".into(),
        },
        &paths,
        regs,
    );
    result.unwrap();
    assert_eq!(err, "warning: profile iş uses this plugin\n");
    assert!(install::installed(&paths.plugin_dir()).is_empty());
}

#[test]
fn plugin_remove_without_profile_users_is_silent() {
    let dir = tempfile::tempdir().unwrap();
    let paths = test_paths(dir.path());
    let plugins = paths.plugin_dir();
    std::fs::create_dir_all(&plugins).unwrap();
    std::fs::write(plugins.join("redmine"), BIN).unwrap();
    std::fs::write(plugins.join("redmine.toml"), "version = \"0.1.0\"\n").unwrap();
    let (result, _, err) = run(
        PluginCommand::Remove {
            name: "redmine".into(),
        },
        &paths,
        vec![],
    );
    result.unwrap();
    assert!(err.is_empty());
}

#[test]
fn cli_parses_subcommands() {
    let cli = Cli::try_parse_from([
        "focusdeck",
        "--profile",
        "is",
        "plugin",
        "install",
        "redmine",
    ])
    .unwrap();
    assert_eq!(cli.profile.as_deref(), Some("is"));
    assert!(matches!(
        cli.command,
        Some(Command::Plugin { action: PluginCommand::Install { ref name } }) if name == "redmine"
    ));
    let cli = Cli::try_parse_from(["focusdeck", "capture", "yarın", "ara"]).unwrap();
    assert!(
        matches!(cli.command, Some(Command::Capture { ref text }) if text == &["yarın", "ara"])
    );
    let cli = Cli::try_parse_from(["focusdeck", "remind", "--send"]).unwrap();
    assert!(matches!(cli.command, Some(Command::Remind { send: true })));
    let cli = Cli::try_parse_from(["focusdeck", "plugin", "list", "--profile", "ev"]).unwrap();
    assert_eq!(cli.profile.as_deref(), Some("ev"));
    assert!(
        Cli::try_parse_from(["focusdeck"])
            .unwrap()
            .command
            .is_none()
    );
}

#[tokio::test]
async fn rendered_errors_carry_no_control_characters() {
    let reg = tempfile::tempdir().unwrap();
    let plugins = tempfile::tempdir().unwrap();
    for url in [
        "ftp://\\u001b[2J",
        "file://\\u001b[2J",
        "file:///\\u001b[2J/yok",
    ] {
        std::fs::write(
            reg.path().join("index.toml"),
            format!(
                "[[plugin]]\nname = \"redmine\"\ndescription = \"d\"\nversion = \"0.1.0\"\nprotocol = 1\n[plugin.targets.{}]\nurl = \"{url}\"\nsha256 = \"{}\"\n",
                current_target(),
                sha256_hex(BIN)
            ),
        )
        .unwrap();
        let err = install::install(&[reg_url(reg.path())], "redmine", plugins.path())
            .await
            .unwrap_err();
        assert!(
            format!("{err:#}").contains('\u{1b}'),
            "{url}: kaynak hata kaçışı içermeli"
        );
        let text = render_error(&err);
        assert!(!text.chars().any(char::is_control), "{url}: {text:?}");
    }

    std::fs::write(reg.path().join("index.toml"), "x = \"\u{1b}[2J\nbozuk\n").unwrap();
    let err = install::install(&[reg_url(reg.path())], "redmine", plugins.path())
        .await
        .unwrap_err();
    assert!(!render_error(&err).chars().any(char::is_control));
}
