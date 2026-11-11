use anyhow::{Context, Result, ensure};
use std::{
    path::{Path, PathBuf},
    process::Command,
};

pub fn find_claude() -> Option<PathBuf> {
    find_in(&search_directories(), "claude")
}

/// Codex CLI from PATH, npm or the Codex desktop app's bundled binary.
pub fn find_codex() -> Option<PathBuf> {
    if let Some(found) = find_in(&search_directories(), "codex") {
        return Some(found);
    }
    // The Codex desktop app keeps versioned copies in hashed folders; use the newest.
    #[cfg(windows)]
    {
        let bin = PathBuf::from(std::env::var_os("LOCALAPPDATA")?).join("OpenAI/Codex/bin");
        std::fs::read_dir(bin)
            .ok()?
            .filter_map(|entry| Some(entry.ok()?.path().join("codex.exe")))
            .filter(|path| is_executable(path))
            .max_by_key(|path| path.metadata().and_then(|m| m.modified()).ok())
    }
    #[cfg(target_os = "macos")]
    {
        Some(PathBuf::from(
            "/Applications/Codex.app/Contents/Resources/codex",
        ))
        .filter(|path| is_executable(path))
    }
    #[cfg(not(any(windows, target_os = "macos")))]
    None
}

fn search_directories() -> Vec<PathBuf> {
    let directories: Vec<PathBuf> = std::env::var_os("PATH")
        .map(|path| std::env::split_paths(&path).collect())
        .unwrap_or_default();
    #[cfg(target_os = "macos")]
    let directories = {
        let mut directories = directories;
        // Finder launches do not inherit the user's interactive shell PATH.
        if let Some(home) = std::env::var_os("HOME") {
            let home = PathBuf::from(home);
            for suffix in [
                ".local/bin",
                ".npm-global/bin",
                ".volta/bin",
                ".local/share/mise/shims",
            ] {
                directories.push(home.join(suffix));
            }
        }
        directories.extend([
            PathBuf::from("/opt/homebrew/bin"),
            PathBuf::from("/usr/local/bin"),
        ]);
        directories
    };
    directories
}

fn find_in(directories: &[PathBuf], program: &str) -> Option<PathBuf> {
    #[cfg(windows)]
    let names = [
        format!("{program}.exe"),
        format!("{program}.cmd"),
        format!("{program}.bat"),
    ];
    #[cfg(not(windows))]
    let names = [program.to_owned()];
    directories
        .iter()
        .filter(|p| p.is_absolute())
        .flat_map(|p| names.iter().map(move |name| p.join(name)))
        .find(|path| is_executable(path))
}

fn is_executable(path: &Path) -> bool {
    let Ok(metadata) = path.metadata() else {
        return false;
    };
    if !metadata.is_file() {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        metadata.permissions().mode() & 0o111 != 0
    }
    #[cfg(not(unix))]
    {
        true
    }
}

pub fn open_quick() -> Result<()> {
    #[cfg(windows)]
    {
        let base = std::env::var_os("LOCALAPPDATA").context("无法找到 Quick 安装目录。")?;
        let exe = PathBuf::from(base).join("Programs/quickwork/Amazon Quick.exe");
        ensure!(exe.is_file(), "没有找到 Amazon Quick，请先安装并登录。");
        Command::new(exe)
            .spawn()
            .context("无法打开 Amazon Quick。")?;
    }
    #[cfg(target_os = "macos")]
    {
        let status = Command::new("/usr/bin/open")
            .args(["-b", "com.amazon.QuickWork.mac"])
            .status()
            .context("无法打开 Amazon Quick。")?;
        ensure!(status.success(), "没有找到 Amazon Quick，请先安装并登录。");
    }
    #[cfg(not(any(windows, target_os = "macos")))]
    anyhow::bail!("当前仅支持 Windows 和 macOS。");
    Ok(())
}

/// Whether CC Switch has registered its `ccswitch://` link handler.
#[cfg(windows)]
pub fn cc_switch_registered() -> bool {
    use std::os::windows::process::CommandExt;
    // reg.exe avoids a registry crate; CREATE_NO_WINDOW keeps a console from flashing.
    Command::new("reg")
        .args(["query", r"HKCR\ccswitch", "/v", "URL Protocol"])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .creation_flags(0x0800_0000)
        .status()
        .is_ok_and(|status| status.success())
}

pub fn shell_quote(value: &str) -> String {
    #[cfg(windows)]
    let escaped = value.replace('\'', "''");
    #[cfg(not(windows))]
    let escaped = value.replace('\'', "'\"'\"'");
    format!("'{escaped}'")
}

pub fn connection_command(settings: &Path, model: &str) -> String {
    format!(
        "claude --settings {} --model {}",
        shell_quote(&settings.to_string_lossy()),
        shell_quote(model)
    )
}

#[cfg(windows)]
pub fn launch_terminal(
    executable: &Path,
    project: &Path,
    settings: &Path,
    model: &str,
) -> Result<()> {
    use std::os::windows::process::CommandExt;
    let config: serde_json::Value = serde_json::from_slice(&std::fs::read(settings)?)?;
    let mut command = Command::new("powershell.exe");
    command.args(["-NoLogo", "-NoProfile", "-NoExit", "-Command",
        "& $env:QUICKWAY_CLAUDE_EXE --settings $env:QUICKWAY_CLAUDE_SETTINGS --model $env:QUICKWAY_SELECTED_MODEL"])
        .current_dir(project)
        .env("QUICKWAY_CLAUDE_EXE", executable)
        .env("QUICKWAY_CLAUDE_SETTINGS", settings)
        .env("QUICKWAY_SELECTED_MODEL", model)
        // A user-level API key would take precedence over the QuickWay token.
        .env_remove("ANTHROPIC_API_KEY")
        .creation_flags(0x00000010);
    if let Some(env) = config["env"].as_object() {
        for (key, value) in env {
            if let Some(value) = value.as_str() {
                command.env(key, value);
            }
        }
    }
    command.spawn().context("无法启动 Claude Code 终端。")?;
    Ok(())
}

#[cfg(target_os = "macos")]
fn terminal_script(executable: &Path, project: &Path, settings: &Path, model: &str) -> String {
    let quote_path = |p: &Path| shell_quote(&p.to_string_lossy());
    // The executable's directory also provides node for npm-installed Claude Code.
    // All paths are single-quoted shell arguments; secrets stay in the private settings file.
    format!(
        "#!/bin/zsh\n/bin/rm -f -- \"$0\"\nexport PATH={}:\"$PATH\":/opt/homebrew/bin:/usr/local/bin:/usr/bin:/bin\nunset ANTHROPIC_API_KEY\ncd -- {} || exit 1\nexec {} --settings {} --model {}\n",
        quote_path(executable.parent().unwrap_or(Path::new("/usr/bin"))),
        quote_path(project),
        quote_path(executable),
        quote_path(settings),
        shell_quote(model)
    )
}

#[cfg(target_os = "macos")]
pub fn launch_terminal(
    executable: &Path,
    project: &Path,
    settings: &Path,
    model: &str,
) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let launcher = settings
        .parent()
        .context("连接配置路径无效。")?
        .join(format!("launch-{}.command", uuid::Uuid::new_v4()));
    quickway::local_files::write_private(
        &launcher,
        terminal_script(executable, project, settings, model).as_bytes(),
    )?;
    std::fs::set_permissions(&launcher, std::fs::Permissions::from_mode(0o700))?;
    // Opening a .command file needs no Apple Events permission to control Terminal.
    let result = Command::new("/usr/bin/open")
        .args(["-a", "Terminal"])
        .arg(&launcher)
        .status();
    if !result.as_ref().is_ok_and(|s| s.success()) {
        let _ = std::fs::remove_file(&launcher);
        anyhow::bail!("无法打开 Terminal，请检查系统中的终端应用。");
    }
    Ok(())
}

#[cfg(not(any(windows, target_os = "macos")))]
pub fn launch_terminal(_: &Path, _: &Path, _: &Path, _: &str) -> Result<()> {
    anyhow::bail!("当前仅支持 Windows 和 macOS。")
}

/// Runs `codex debug models --bundled`: the model catalog shipped with the
/// installed Codex, including each model's system prompt.
pub fn codex_bundled_models(executable: &Path) -> Option<serde_json::Value> {
    let mut command = Command::new(executable);
    command
        .args(["debug", "models", "--bundled"])
        .stdin(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x0800_0000);
    }
    let output = command.output().ok()?;
    if !output.status.success() {
        return None;
    }
    serde_json::from_slice(&output.stdout).ok()
}

/// Opens Codex in its own terminal. `args` are passed as separate arguments
/// and must not contain newlines or double quotes; the API key is read from
/// `key_file` into `QUICKWAY_API_KEY`, which the provider's `env_key` names.
#[cfg(windows)]
pub fn launch_codex_terminal(
    executable: &Path,
    project: &Path,
    args: &[String],
    key_file: &Path,
) -> Result<()> {
    use std::os::windows::process::CommandExt;
    ensure!(
        args.iter().all(|a| !a.contains(['\n', '"'])),
        "Codex 启动参数无效。"
    );
    let key = std::fs::read_to_string(key_file).context("无法读取本地代理密钥。")?;
    codex_powershell(executable, project, args, key.trim(), true)
        .creation_flags(0x00000010)
        .spawn()
        .context("无法启动 Codex 终端。")?;
    Ok(())
}

/// The PowerShell command that runs Codex. Arguments travel in one
/// newline-separated variable so no value is reparsed by the shell.
#[cfg(windows)]
fn codex_powershell(
    executable: &Path,
    project: &Path,
    args: &[String],
    key: &str,
    keep_open: bool,
) -> Command {
    let mut command = Command::new("powershell.exe");
    command.args(["-NoLogo", "-NoProfile"]);
    if keep_open {
        command.arg("-NoExit");
    }
    command
        .args([
            "-Command",
            "& $env:QUICKWAY_CODEX_EXE @($env:QUICKWAY_CODEX_ARGS -split \"`n\")",
        ])
        .current_dir(project)
        .env("QUICKWAY_CODEX_EXE", executable)
        .env("QUICKWAY_CODEX_ARGS", args.join("\n"))
        .env("QUICKWAY_API_KEY", key);
    command
}

/// Runs the real launch command with `codex exec` against a live proxy.
/// QUICKWAY_TEST_PORT / QUICKWAY_TEST_KEY name the proxy and its key.
#[cfg(all(test, windows))]
pub fn codex_exec_for_test(
    executable: &Path,
    project: &Path,
    args: &[String],
    key: &str,
) -> std::process::Output {
    codex_powershell(executable, project, args, key, false)
        .stdin(std::process::Stdio::null())
        .output()
        .unwrap()
}

#[cfg(target_os = "macos")]
fn codex_script(executable: &Path, project: &Path, args: &[String], key_file: &Path) -> String {
    let quote_path = |p: &Path| shell_quote(&p.to_string_lossy());
    let args: Vec<String> = args.iter().map(|a| shell_quote(a)).collect();
    // The key is read from the private key file, so this script holds no secret.
    format!(
        "#!/bin/zsh\n/bin/rm -f -- \"$0\"\nexport PATH={}:\"$PATH\":/opt/homebrew/bin:/usr/local/bin:/usr/bin:/bin\nexport QUICKWAY_API_KEY=\"$(/bin/cat -- {})\"\ncd -- {} || exit 1\nexec {} {}\n",
        quote_path(executable.parent().unwrap_or(Path::new("/usr/bin"))),
        quote_path(key_file),
        quote_path(project),
        quote_path(executable),
        args.join(" ")
    )
}

#[cfg(target_os = "macos")]
pub fn launch_codex_terminal(
    executable: &Path,
    project: &Path,
    args: &[String],
    key_file: &Path,
) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let launcher = key_file
        .parent()
        .context("配置目录无效。")?
        .join(format!("launch-codex-{}.command", uuid::Uuid::new_v4()));
    quickway::local_files::write_private(
        &launcher,
        codex_script(executable, project, args, key_file).as_bytes(),
    )?;
    std::fs::set_permissions(&launcher, std::fs::Permissions::from_mode(0o700))?;
    let result = Command::new("/usr/bin/open")
        .args(["-a", "Terminal"])
        .arg(&launcher)
        .status();
    if !result.as_ref().is_ok_and(|s| s.success()) {
        let _ = std::fs::remove_file(&launcher);
        anyhow::bail!("无法打开 Terminal，请检查系统中的终端应用。");
    }
    Ok(())
}

#[cfg(not(any(windows, target_os = "macos")))]
pub fn launch_codex_terminal(_: &Path, _: &Path, _: &[String], _: &Path) -> Result<()> {
    anyhow::bail!("当前仅支持 Windows 和 macOS。")
}

#[cfg(all(test, target_os = "macos"))]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    #[test]
    #[ignore = "opens a real Terminal window with a synthetic executable"]
    fn native_terminal_open_runs_the_launcher() {
        let root =
            std::env::temp_dir().join(format!("QuickWay 中文 '测试' {}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let executable = root.join("claude");
        std::fs::write(
            &executable,
            "#!/bin/sh\nprintf '%s\\n' \"$PWD\" \"$@\" > terminal-result.txt\n",
        )
        .unwrap();
        std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o700)).unwrap();
        let settings = root.join("connection.json");
        std::fs::write(&settings, "{}").unwrap();
        launch_terminal(&executable, &root, &settings, "claude-opus-4-8").unwrap();
        let receipt = root.join("terminal-result.txt");
        for _ in 0..100 {
            if receipt.exists() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
        assert_eq!(
            std::fs::read_to_string(&receipt).expect("Terminal did not run the .command file"),
            format!(
                "{}\n--settings\n{}\n--model\nclaude-opus-4-8\n",
                root.display(),
                settings.display()
            )
        );
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn terminal_preserves_unicode_quotes_spaces_and_shell_metacharacters() {
        let root = std::env::temp_dir().join(format!("quickway-{}", uuid::Uuid::new_v4()));
        let project = root.join("中文 项目'$(touch INJECTED)`touch INJECTED`\n第二行");
        std::fs::create_dir_all(&project).unwrap();
        let executable = root.join("claude");
        std::fs::write(
            &executable,
            "#!/bin/sh\nprintf '%s\\n' \"$PWD\" \"$@\" > received.txt\n",
        )
        .unwrap();
        std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o700)).unwrap();
        assert_eq!(
            find_in(&[PathBuf::from("relative"), root.clone()], "claude"),
            Some(executable.clone())
        );
        let settings = root.join("连接 '配置.json");
        let launcher = root.join("launch.command");
        std::fs::write(
            &launcher,
            terminal_script(&executable, &project, &settings, "claude-opus-4-8"),
        )
        .unwrap();
        assert!(
            Command::new("/bin/zsh")
                .arg(&launcher)
                .status()
                .unwrap()
                .success()
        );
        assert!(!launcher.exists());
        assert!(!project.join("INJECTED").exists());
        let actual = std::fs::read_to_string(project.join("received.txt")).unwrap();
        assert_eq!(
            actual,
            format!(
                "{}\n--settings\n{}\n--model\nclaude-opus-4-8\n",
                project.display(),
                settings.display()
            )
        );
        std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o600)).unwrap();
        assert!(find_in(std::slice::from_ref(&root), "claude").is_none());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn copied_command_round_trips_posix_arguments() {
        let path = Path::new("/Users/中文 用户/it's $(echo wrong).json");
        let command = connection_command(path, "claude-opus-4-8");
        let output = Command::new("/bin/zsh")
            .args([
                "-c",
                &format!("claude() {{ printf '%s\\n' \"$@\"; }}; {command}"),
            ])
            .output()
            .unwrap();
        assert!(output.status.success());
        assert_eq!(
            String::from_utf8(output.stdout).unwrap(),
            format!("--settings\n{}\n--model\nclaude-opus-4-8\n", path.display())
        );
    }
}
