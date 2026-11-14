use std::os::unix::fs::PermissionsExt;
use std::process::Command;

const TOKEN: &str = "0123456789abcdef0123456789abcdef0123456789abcdef";

#[test]
fn unreachable_server_keeps_the_tui_closed_and_creates_no_profile() {
    let dir = tempfile::tempdir().unwrap();
    let dead = {
        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        format!("http://{}", l.local_addr().unwrap())
    };
    let home = dir.path().join("home");
    let config = dir.path().join("config");
    std::fs::create_dir_all(&home).unwrap();
    std::fs::create_dir_all(&config).unwrap();
    let token = home.join("token");
    std::fs::write(&token, format!("{TOKEN}\n")).unwrap();
    std::fs::set_permissions(&token, std::fs::Permissions::from_mode(0o600)).unwrap();
    std::fs::write(
        config.join("config.toml"),
        format!("[server]\nurl = \"{dead}\"\ntoken_file = \"~/token\"\n"),
    )
    .unwrap();

    let out = Command::new(env!("CARGO_BIN_EXE_seenboard"))
        .env("HOME", &home)
        .env("SEENBOARD_CONFIG_DIR", &config)
        .env("SEENBOARD_DATA_DIR", dir.path().join("data"))
        .env("SEENBOARD_STATE_DIR", dir.path().join("state"))
        .env_remove("SEENBOARD_PROFILE")
        .env_remove("SEENBOARD_LANG")
        .env_remove("LANG")
        .env_remove("LC_MESSAGES")
        .env("LC_ALL", "C")
        .output()
        .unwrap();

    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(!out.status.success(), "{stderr}");
    assert!(stderr.contains(&dead), "{stderr}");
    assert!(!stderr.contains(TOKEN), "{stderr}");
    let profiles = config.join("profiles");
    let written = std::fs::read_dir(&profiles).map_or(0, |d| d.count());
    assert_eq!(written, 0, "client mode must not write a starter profile");
}
