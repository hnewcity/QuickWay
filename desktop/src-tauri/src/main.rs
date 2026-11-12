#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use anyhow::{Context, Result, anyhow, ensure};
use futures_util::StreamExt;
mod platform;

use quickway::thinking::Effort;
use quickway::{auth, server, upstream::Upstream};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::Duration,
};
use tauri::{
    Manager, State,
    ipc::Channel,
    menu::{Menu, MenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
};
use tauri_plugin_clipboard_manager::ClipboardExt;
use tauri_plugin_opener::OpenerExt;
use tauri_plugin_updater::UpdaterExt;
use tokio::sync::{Mutex as AsyncMutex, oneshot};

#[derive(Clone, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
struct Preferences {
    port: u16,
    model: String,
    project_dir: String,
    auto_start: bool,
    thinking_effort: Effort,
    /// Codex keeps its own model and effort; an empty model uses `model`.
    codex_model: String,
    codex_effort: Effort,
}
impl Default for Preferences {
    fn default() -> Self {
        Self {
            port: 8741,
            model: "quick-smart".into(),
            project_dir: String::new(),
            auto_start: true,
            thinking_effort: Effort::Off,
            codex_model: String::new(),
            codex_effort: Effort::Medium,
        }
    }
}
impl Preferences {
    fn codex_model(&self) -> &str {
        if self.codex_model.is_empty() {
            &self.model
        } else {
            &self.codex_model
        }
    }
}
struct RunningServer {
    stop: oneshot::Sender<()>,
    task: tauri::async_runtime::JoinHandle<()>,
}
struct Desktop {
    config_dir: PathBuf,
    preferences: Mutex<Preferences>,
    app: Arc<server::App>,
    running: AsyncMutex<Option<RunningServer>>,
    cancellations: Mutex<HashMap<String, oneshot::Sender<()>>>,
    client: reqwest::Client,
}
impl Desktop {
    fn claude_settings(&self) -> Result<PathBuf> {
        let preferences = self.preferences();
        let path = self.config_dir.join(format!(
            "claude-connection-{}.json",
            preferences.thinking_effort.as_str()
        ));
        let config = claude_connection(&preferences, &self.app.key);
        quickway::local_files::write_private(&path, &serde_json::to_vec_pretty(&config)?)
            .context("无法写入 Claude Code 连接配置。")?;
        Ok(path)
    }

    fn preferences(&self) -> Preferences {
        self.preferences.lock().unwrap().clone()
    }

    async fn start(&self) -> Result<()> {
        let mut running = self.running.lock().await;
        if running.is_some() {
            return Ok(());
        }
        let port = self.preferences().port;
        let listener = tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, port))
            .await
            .map_err(|_| {
                anyhow!("端口 {port} 已被占用。请停止其他代理，或在连接设置中修改端口。")
            })?;
        let router = server::router(self.app.clone());
        let (stop, receiver) = oneshot::channel();
        let task = tauri::async_runtime::spawn(async move {
            let _ = axum::serve(listener, router)
                .with_graceful_shutdown(async {
                    let _ = receiver.await;
                })
                .await;
        });
        *running = Some(RunningServer { stop, task });
        Ok(())
    }

    async fn stop(&self) {
        for (_, cancel) in self.cancellations.lock().unwrap().drain() {
            let _ = cancel.send(());
        }
        let mut running = self.running.lock().await;
        if let Some(RunningServer { stop, mut task }) = running.take() {
            let _ = stop.send(());
            self.app.upstream.cloud.shutdown().await;
            if tokio::time::timeout(Duration::from_secs(3), &mut task)
                .await
                .is_err()
            {
                task.abort();
            }
        }
    }
}

fn claude_connection(preferences: &Preferences, key: &str) -> Value {
    let effort = preferences.thinking_effort;
    let enabled = effort != Effort::Off;
    let mut config = json!({"alwaysThinkingEnabled":enabled,"effortLevel":if enabled {effort.as_str()} else {"low"},"env": {
        "ANTHROPIC_BASE_URL": format!("http://127.0.0.1:{}", preferences.port),
        // Only the bearer token: also setting ANTHROPIC_API_KEY makes Claude
        // Code warn that both are set and may ask to approve the key.
        "ANTHROPIC_AUTH_TOKEN": key,
        "ANTHROPIC_CUSTOM_HEADERS": format!("x-quickway-thinking-effort: {}", effort.as_str()),
        "CLAUDE_CODE_EFFORT_LEVEL": if enabled { effort.as_str() } else { "low" },
        "MAX_THINKING_TOKENS": effort.budget().to_string(),
        "ANTHROPIC_DEFAULT_OPUS_MODEL": preferences.model,
        "ANTHROPIC_DEFAULT_SONNET_MODEL": preferences.model,
        "ANTHROPIC_DEFAULT_HAIKU_MODEL": preferences.model,
        "ANTHROPIC_SMALL_FAST_MODEL": preferences.model,
        "CLAUDE_CODE_DISABLE_EXPERIMENTAL_BETAS": "1",
        "ENABLE_TOOL_SEARCH": "false"
    }});
    if let Some(mode) = quickway::cloud::mode(&preferences.model) {
        // This is a client compaction budget for a routed tier, not a promise
        // about the cloud's serving model or its native context limit.
        config["env"]["CLAUDE_CODE_MAX_CONTEXT_TOKENS"] = "200000".into();
        config["env"]["ANTHROPIC_CUSTOM_MODEL_OPTION"] = preferences.model.clone().into();
        config["env"]["ANTHROPIC_CUSTOM_MODEL_OPTION_NAME"] =
            format!("Quick {mode} (cloud tier)").into();
    }
    config
}

/// Codex's own effort name for a QuickWay level.
fn codex_effort(effort: Effort) -> &'static str {
    match effort {
        Effort::Off => "none",
        other => other.as_str(),
    }
}

/// A TOML string for a `-c key=value` override, single-quoted so the
/// argument never needs double quotes (PowerShell 5.1 mangles those).
fn toml_literal(value: &str) -> Option<String> {
    (!value.contains(['\'', '\n', '\r', '"'])).then(|| format!("'{value}'"))
}

/// `-c` overrides that point Codex at QuickWay for one launch, leaving
/// `~/.codex/config.toml` untouched. The key comes from `QUICKWAY_API_KEY`.
fn codex_args(preferences: &Preferences, catalog: Option<&Path>) -> Vec<String> {
    let effort = preferences.codex_effort;
    let mut overrides = vec![
        "model_provider='quickway'".to_owned(),
        "model_providers.quickway.name='QuickWay'".to_owned(),
        format!(
            "model_providers.quickway.base_url='http://127.0.0.1:{}/v1'",
            preferences.port
        ),
        "model_providers.quickway.wire_api='responses'".to_owned(),
        "model_providers.quickway.env_key='QUICKWAY_API_KEY'".to_owned(),
        // The proxy applies this level whatever Codex puts in the request.
        format!(
            "model_providers.quickway.http_headers.x-quickway-thinking-effort='{}'",
            effort.as_str()
        ),
        format!("model_reasoning_effort='{}'", codex_effort(effort)),
    ];
    if let Some(path) = catalog.and_then(|p| toml_literal(&p.to_string_lossy())) {
        overrides.push(format!("model_catalog_json={path}"));
    }
    let mut args = vec!["-m".to_owned(), preferences.codex_model().to_owned()];
    for value in overrides {
        args.push("-c".into());
        args.push(value);
    }
    args
}

/// Codex knows OpenAI models only. Without metadata it falls back to a
/// reduced toolset (no apply_patch), so models it lacks get a copy of its
/// first listed model's entry. Bundled entries are kept unchanged.
fn codex_catalog(bundled: &Value, models: &[quickway::models::Model]) -> Option<Value> {
    let entries = bundled["models"].as_array()?;
    let template = entries
        .iter()
        .find(|m| m["visibility"] == "list")
        .or_else(|| entries.first())?;
    let mut catalog = entries.clone();
    for (index, model) in models.iter().enumerate() {
        if entries.iter().any(|m| m["slug"] == model.id.as_str()) {
            continue;
        }
        let mut entry = template.clone();
        entry["slug"] = json!(model.id);
        entry["display_name"] = json!(model.name);
        entry["description"] = json!(format!("{} via QuickWay", model.name));
        entry["priority"] = json!(1000 + index);
        // Claude and other non-OpenAI models are sized for a 200K context.
        entry["context_window"] = json!(200_000);
        entry["max_context_window"] = json!(200_000);
        for field in ["upgrade", "availability_nux", "auto_compact_token_limit"] {
            entry.as_object_mut()?.remove(field);
        }
        if let Some(prompt) = entry["base_instructions"].as_str() {
            entry["base_instructions"] = json!(rename_base_model(prompt, &model.name));
        }
        catalog.push(entry);
    }
    Some(json!({ "models": catalog }))
}

/// Rewrites "an agent based on GPT-6" in a copied prompt to name the real model.
fn rename_base_model(prompt: &str, name: &str) -> String {
    const PREFIX: &str = "an agent based on ";
    let Some(start) = prompt.find(PREFIX).map(|i| i + PREFIX.len()) else {
        return prompt.to_owned();
    };
    let rest = &prompt[start..];
    let token = rest
        .find(|c: char| !(c.is_ascii_alphanumeric() || c == '-' || c == '.'))
        .unwrap_or(rest.len());
    let token = rest[..token].trim_end_matches('.').len();
    format!("{}{name}{}", &prompt[..start], &rest[token..])
}

fn codex_config(preferences: &Preferences) -> String {
    format!(
        "# ~/.codex/config.toml\nmodel_provider = \"quickway\"\nmodel = \"{}\"\nmodel_reasoning_effort = \"{}\"\n\n[model_providers.quickway]\nname = \"QuickWay\"\nbase_url = \"http://127.0.0.1:{}/v1\"\nwire_api = \"responses\"\n# Set QUICKWAY_API_KEY to the QuickWay API key before running codex.\nenv_key = \"QUICKWAY_API_KEY\"\n",
        preferences.codex_model(),
        codex_effort(preferences.codex_effort),
        preferences.port
    )
}

/// One-click provider import for CC Switch. The link carries the local key, so it is
/// built here and handed straight to the OS instead of passing through the webview.
fn cc_switch_link(preferences: &Preferences, key: &str) -> String {
    let config = claude_connection(preferences, key);
    let env = |name: &str| config["env"][name].as_str().unwrap_or_default().to_owned();
    let mut link = reqwest::Url::parse("ccswitch://v1/import").expect("static CC Switch link");
    link.query_pairs_mut()
        .append_pair("resource", "provider")
        .append_pair("app", "claude")
        .append_pair("name", "QuickWay")
        .append_pair("endpoint", &env("ANTHROPIC_BASE_URL"))
        .append_pair("apiKey", key)
        .append_pair("model", &preferences.model)
        .append_pair("opusModel", &env("ANTHROPIC_DEFAULT_OPUS_MODEL"))
        .append_pair("sonnetModel", &env("ANTHROPIC_DEFAULT_SONNET_MODEL"))
        .append_pair("haikuModel", &env("ANTHROPIC_DEFAULT_HAIKU_MODEL"))
        .append_pair("notes", "本机 QuickWay 代理。使用时请保持 QuickWay 运行。");
    link.into()
}

#[tauri::command]
async fn import_cc_switch(
    app: tauri::AppHandle,
    state: State<'_, Desktop>,
) -> std::result::Result<(), String> {
    let preferences = state.preferences();
    state
        .app
        .upstream
        .models(false)
        .await
        .resolve(&preferences.model)?;
    let link = cc_switch_link(&preferences, &state.app.key);
    let missing = "没有找到 CC Switch。请先安装 CC Switch，再点导入。";
    #[cfg(target_os = "macos")]
    {
        // `open` exits non-zero when no app handles the scheme; the opener plugin would not report it.
        let _ = &app;
        let status = std::process::Command::new("/usr/bin/open")
            .arg(&link)
            .status()
            .map_err(|_| missing.to_string())?;
        if status.success() {
            Ok(())
        } else {
            Err(missing.into())
        }
    }
    #[cfg(not(target_os = "macos"))]
    {
        // Windows would otherwise offer to search the Store for an unknown scheme.
        #[cfg(windows)]
        if !platform::cc_switch_registered() {
            return Err(missing.into());
        }
        app.opener()
            .open_url(link, None::<&str>)
            .map_err(|_| missing.into())
    }
}

#[tauri::command]
fn api_key(state: State<'_, Desktop>) -> String {
    state.app.key.clone()
}

#[tauri::command]
async fn get_status(state: State<'_, Desktop>) -> std::result::Result<Value, String> {
    let catalog = state.app.upstream.models(false).await;
    let preferences = state.preferences();
    let upstream = &state.app.upstream;
    let credential_source = upstream.sessions.credential_source().await;
    let account = match upstream.sessions.session(&upstream.client).await {
        Ok(session) => {
            json!({"signedIn":true,"canRefresh":!session.managed_by_quick && !session.refresh_token.is_empty() && session.refresh_token_expiry > auth::now() && session.client_secret_expires_at > auth::now(),"error":null,
                "expiresAt":session.access_token_expiry,"enterprise":session.managed_by_quick})
        }
        Err(error) => json!({"signedIn":false,"canRefresh":false,"error":error.to_string()}),
    };
    let last_cloud_model = upstream.last_cloud_model().await;
    Ok(json!({
        "preferences":preferences,"running":state.running.lock().await.is_some(),
        "account":account,"models":catalog.models,"modelCatalog":{"source":catalog.source,"lastUpdated":catalog.last_updated,"error":catalog.error},"claudeInstalled":platform::find_claude().is_some(),"codexInstalled":platform::find_codex().is_some(),
        "credentials":{"source":credential_source},
        "lastCloudModel":last_cloud_model,
        "version":env!("CARGO_PKG_VERSION")
    }))
}

#[tauri::command]
async fn refresh_models(
    state: State<'_, Desktop>,
) -> std::result::Result<quickway::models::Catalog, String> {
    Ok(state.app.upstream.models(true).await)
}

#[tauri::command]
async fn start_proxy(state: State<'_, Desktop>) -> std::result::Result<(), String> {
    state.start().await.map_err(|e| e.to_string())
}

#[tauri::command]
async fn authorize_quick(state: State<'_, Desktop>) -> std::result::Result<(), String> {
    let upstream = &state.app.upstream;
    upstream
        .sessions
        .authorize(&upstream.client)
        .await
        .map(|_| ())
        .map_err(|error| error.to_string())
}

#[tauri::command]
async fn export_credentials(
    state: State<'_, Desktop>,
    path: PathBuf,
) -> std::result::Result<(), String> {
    let upstream = &state.app.upstream;
    upstream
        .sessions
        .export_credentials(&path, &upstream.client)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn import_credentials(
    state: State<'_, Desktop>,
    path: PathBuf,
) -> std::result::Result<(), String> {
    let upstream = &state.app.upstream;
    upstream
        .sessions
        .import_credentials(path)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn forget_credentials(state: State<'_, Desktop>) -> std::result::Result<(), String> {
    state
        .app
        .upstream
        .sessions
        .forget_credentials()
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn stop_proxy(state: State<'_, Desktop>) -> std::result::Result<(), String> {
    state.stop().await;
    Ok(())
}

#[tauri::command]
async fn save_preferences(
    state: State<'_, Desktop>,
    mut preferences: Preferences,
) -> std::result::Result<(), String> {
    if preferences.port < 1024 {
        return Err("请输入 1024–65535 之间的端口。".into());
    }
    // Keep an unavailable historical selection when changing unrelated settings.
    // A deliberate model change must resolve against the current directory.
    if preferences.model != state.preferences().model {
        preferences.model = state
            .app
            .upstream
            .models(false)
            .await
            .resolve(&preferences.model)?
            .0;
    }
    if !preferences.codex_model.is_empty()
        && preferences.codex_model != state.preferences().codex_model
    {
        preferences.codex_model = state
            .app
            .upstream
            .models(false)
            .await
            .resolve(&preferences.codex_model)?
            .0;
    }
    // Serialize the check and update with start(), which reads the port while
    // holding this same lock. Otherwise a fast start can bind the previous port.
    let running = state.running.lock().await;
    if preferences.port != state.preferences().port && running.is_some() {
        return Err("请先停止代理，再修改端口。".into());
    }
    let path = state.config_dir.join("settings.json");
    let temporary = state.config_dir.join("settings.tmp");
    std::fs::write(&temporary, serde_json::to_vec_pretty(&preferences).unwrap())
        .map_err(|_| "无法保存设置。")?;
    std::fs::rename(&temporary, &path).map_err(|_| "无法更新设置文件。")?;
    *state.preferences.lock().unwrap() = preferences;
    Ok(())
}

#[tauri::command]
async fn refresh_usage(state: State<'_, Desktop>) -> std::result::Result<Value, String> {
    state.app.upstream.usage().await.map_err(|e| e.to_string())
}

/// Redacted support report, copied by the user and sent to whoever helps them.
#[tauri::command]
async fn diagnostics(state: State<'_, Desktop>) -> std::result::Result<String, String> {
    let mut report =
        quickway::diagnostics::report(&state.app.upstream, env!("CARGO_PKG_VERSION"), true).await;
    report["desktop"] = json!({
        "proxy_running": state.running.lock().await.is_some(),
        "port": state.preferences().port,
        "claude_installed": platform::find_claude().is_some(),
        "codex_installed": platform::find_codex().is_some(),
    });
    serde_json::to_string_pretty(&report).map_err(|e| e.to_string())
}

#[tauri::command]
fn open_quick() -> std::result::Result<(), String> {
    platform::open_quick().map_err(|error| error.to_string())
}

#[tauri::command]
async fn copy_connection(
    app: tauri::AppHandle,
    state: State<'_, Desktop>,
    key_only: bool,
) -> std::result::Result<(), String> {
    let preferences = state.preferences();
    let text = if key_only {
        state.app.key.clone()
    } else {
        state
            .app
            .upstream
            .models(false)
            .await
            .resolve(&preferences.model)?;
        let settings = state.claude_settings().map_err(|e| e.to_string())?;
        platform::connection_command(&settings, &preferences.model)
    };
    app.clipboard()
        .write_text(text)
        .map_err(|_| "无法写入剪贴板。".into())
}

/// Native tray menu items, kept so the frontend can relabel them in the chosen UI language.
struct TrayMenu {
    show: MenuItem<tauri::Wry>,
    quit: MenuItem<tauri::Wry>,
}
const TRAY_ID: &str = "main";

#[tauri::command]
fn set_tray_language(
    app: tauri::AppHandle,
    menu: State<'_, TrayMenu>,
    show: String,
    quit: String,
    tooltip: String,
) -> std::result::Result<(), String> {
    menu.show.set_text(show).map_err(|e| e.to_string())?;
    menu.quit.set_text(quit).map_err(|e| e.to_string())?;
    if let Some(tray) = app.tray_by_id(TRAY_ID) {
        tray.set_tooltip(Some(tooltip)).map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[tauri::command]
fn copy_text(app: tauri::AppHandle, text: String) -> std::result::Result<(), String> {
    app.clipboard()
        .write_text(text)
        .map_err(|_| "无法写入剪贴板。".into())
}

#[tauri::command]
fn open_external(app: tauri::AppHandle, url: String) -> std::result::Result<(), String> {
    let parsed = reqwest::Url::parse(&url).map_err(|_| "链接格式无效。")?;
    if !["https", "http"].contains(&parsed.scheme()) {
        return Err("只支持网页链接。".into());
    }
    app.opener()
        .open_url(url, None::<&str>)
        .map_err(|_| "无法打开链接。".into())
}

const RELEASES_URL: &str = "https://github.com/hnewcity/QuickWay/releases/latest";

/// Self-update replaces the installed app in place. The portable Windows exe, an app run from the
/// mounted DMG or a translocated quarantine copy cannot be replaced, so those get a download link.
fn replaceable_install(exe: &Path, windows: bool) -> bool {
    if windows {
        return exe
            .parent()
            .is_some_and(|dir| dir.join("uninstall.exe").is_file());
    }
    let path = exe.to_string_lossy();
    path.contains(".app/Contents/MacOS/")
        && !path.starts_with("/Volumes/")
        && !path.contains("/AppTranslocation/")
}

fn self_update_supported() -> bool {
    !cfg!(debug_assertions)
        && (cfg!(target_os = "windows") || cfg!(target_os = "macos"))
        && std::env::current_exe()
            .is_ok_and(|exe| replaceable_install(&exe, cfg!(target_os = "windows")))
}

async fn pending_update(
    app: &tauri::AppHandle,
) -> std::result::Result<Option<tauri_plugin_updater::Update>, String> {
    app.updater()
        .map_err(|e| e.to_string())?
        .check()
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn check_update(app: tauri::AppHandle) -> std::result::Result<Value, String> {
    let update = pending_update(&app).await?;
    Ok(json!({
        "current": env!("CARGO_PKG_VERSION"),
        "version": update.as_ref().map(|update| update.version.clone()),
        "notes": update.and_then(|update| update.body),
        "installable": self_update_supported(),
        "url": RELEASES_URL,
    }))
}

/// Downloads and verifies the signed update while the proxy keeps serving, then stops the proxy and
/// installs. Windows hands over to the installer, which restarts the app; macOS relaunches here.
#[tauri::command]
async fn install_update(
    app: tauri::AppHandle,
    state: State<'_, Desktop>,
    on_event: Channel<Value>,
) -> std::result::Result<(), String> {
    if !self_update_supported() {
        return Err("此版本不能自动更新，请下载新版安装包。".into());
    }
    let update = pending_update(&app)
        .await?
        .ok_or("已经是最新版本。")?;
    let mut downloaded = 0u64;
    let bytes = update
        .download(
            |chunk, total| {
                downloaded += chunk as u64;
                let _ = on_event.send(json!({"type":"progress", "downloaded":downloaded, "total":total}));
            },
            || {},
        )
        .await
        .map_err(|e| e.to_string())?;
    let _ = on_event.send(json!({"type":"installing"}));
    state.stop().await;
    update.install(bytes).map_err(|e| e.to_string())?;
    app.restart()
}

#[tauri::command]
async fn launch_claude(state: State<'_, Desktop>) -> std::result::Result<(), String> {
    let preferences = state.preferences();
    state
        .app
        .upstream
        .models(false)
        .await
        .resolve(&preferences.model)?;
    let path = Path::new(&preferences.project_dir)
        .canonicalize()
        .map_err(|_| "请先选择一个有效的项目文件夹。")?;
    if !path.is_dir() {
        return Err("请选择文件夹。".into());
    }
    let executable = platform::find_claude()
        .ok_or("没有检测到 Claude Code。请先安装 Claude Code，再重新打开 QuickWay。")?;
    state.start().await.map_err(|e| e.to_string())?;
    let settings = state.claude_settings().map_err(|e| e.to_string())?;
    platform::launch_terminal(&executable, &path, &settings, &preferences.model)
        .map_err(|error| error.to_string())?;
    Ok(())
}

#[tauri::command]
async fn launch_codex(state: State<'_, Desktop>) -> std::result::Result<(), String> {
    let preferences = state.preferences();
    let catalog = state.app.upstream.models(false).await;
    catalog.resolve(preferences.codex_model())?;
    let path = Path::new(&preferences.project_dir)
        .canonicalize()
        .map_err(|_| "请先选择一个有效的项目文件夹。")?;
    if !path.is_dir() {
        return Err("请选择文件夹。".into());
    }
    let executable = platform::find_codex()
        .ok_or("没有检测到 Codex。请先安装 Codex CLI 或 Codex 应用，再重新打开 QuickWay。")?;
    state.start().await.map_err(|e| e.to_string())?;
    // Without a catalog Codex still runs, with its reduced fallback toolset.
    let models_file = state.config_dir.join("codex-models.json");
    let models_file = tauri::async_runtime::spawn_blocking({
        let executable = executable.clone();
        move || platform::codex_bundled_models(&executable)
    })
    .await
    .ok()
    .flatten()
    .and_then(|bundled| codex_catalog(&bundled, &catalog.models))
    .and_then(|catalog| serde_json::to_vec(&catalog).ok())
    .and_then(|bytes| std::fs::write(&models_file, bytes).ok())
    .map(|_| models_file);
    let args = codex_args(&preferences, models_file.as_deref());
    platform::launch_codex_terminal(
        &executable,
        &path,
        &args,
        &state.config_dir.join("proxy.key"),
    )
    .map_err(|error| error.to_string())?;
    Ok(())
}

#[tauri::command]
fn copy_codex_config(
    app: tauri::AppHandle,
    state: State<'_, Desktop>,
) -> std::result::Result<(), String> {
    app.clipboard()
        .write_text(codex_config(&state.preferences()))
        .map_err(|_| "无法写入剪贴板。".into())
}

#[derive(Clone, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum ChatEvent {
    Delta { text: String },
    Model { served: String },
    Usage { input: u64, output: u64 },
    Done { reason: String },
    Cancelled,
    Error { message: String },
}

fn chat_model_event(event: &Value) -> Option<ChatEvent> {
    quickway::cloud::served_model(&event["quickway"]).map(|served| ChatEvent::Model {
        served: served.to_owned(),
    })
}

async fn receive_chat(
    state: &Desktop,
    model: &str,
    effort: Effort,
    messages: Value,
    output: &Channel<ChatEvent>,
) -> Result<()> {
    ensure!(
        messages
            .as_array()
            .is_some_and(|v| !v.is_empty() && v.len() <= 200),
        "对话为空或过长，请新建对话。"
    );
    let port = state.preferences().port;
    let mut request = json!({"model":model,"messages":messages,"max_tokens":4096,"stream":true});
    effort.apply(&mut request, model).map_err(|e| anyhow!(e))?;
    let response = state
        .client
        .post(format!("http://127.0.0.1:{port}/v1/messages"))
        .header("x-api-key", &state.app.key)
        .json(&request)
        .send()
        .await
        .map_err(|_| anyhow!("无法连接本地代理，请重新启动代理。"))?;
    if !response.status().is_success() {
        let status = response.status();
        let body: Value = response.json().await.unwrap_or(Value::Null);
        return Err(anyhow!(
            "{}",
            body["error"]["message"]
                .as_str()
                .unwrap_or(&format!("请求失败（HTTP {status}）"))
        ));
    }
    let mut stream = response.bytes_stream();
    let mut buffer = Vec::<u8>::new();
    let mut ended = false;
    let mut reason = String::new();
    while let Some(chunk) = stream.next().await {
        buffer.extend_from_slice(&chunk.map_err(|_| anyhow!("连接中断，请重试。"))?);
        ensure!(buffer.len() <= 4 * 1024 * 1024, "响应数据过大。");
        while let Some(end) = buffer.iter().position(|b| *b == b'\n') {
            let line: Vec<_> = buffer.drain(..=end).collect();
            let line = std::str::from_utf8(&line)
                .context("响应编码无效。")?
                .trim_end();
            let Some(data) = line.strip_prefix("data: ") else {
                continue;
            };
            let event: Value = serde_json::from_str(data).context("响应格式无效。")?;
            match event["type"].as_str() {
                Some("content_block_delta") => {
                    if let Some(text) = event["delta"]["text"].as_str() {
                        let _ = output.send(ChatEvent::Delta {
                            text: text.to_owned(),
                        });
                    }
                }
                Some("message_delta") => {
                    if let Some(model) = chat_model_event(&event) {
                        let _ = output.send(model);
                    }
                    reason = event["delta"]["stop_reason"]
                        .as_str()
                        .unwrap_or("end_turn")
                        .to_owned();
                    let _ = output.send(ChatEvent::Usage {
                        input: event["usage"]["input_tokens"].as_u64().unwrap_or(0),
                        output: event["usage"]["output_tokens"].as_u64().unwrap_or(0),
                    });
                }
                Some("message_stop") => ended = true,
                Some("error") => {
                    return Err(anyhow!(
                        "{}",
                        event["error"]["message"]
                            .as_str()
                            .unwrap_or("模型返回了错误，请重试。")
                    ));
                }
                _ => {}
            }
        }
    }
    ensure!(ended, "响应未完整结束，请重试。");
    let _ = output.send(ChatEvent::Done { reason });
    Ok(())
}

#[tauri::command]
async fn chat(
    state: State<'_, Desktop>,
    model: String,
    thinking_effort: Effort,
    messages: Value,
    request_id: String,
    on_event: Channel<ChatEvent>,
) -> std::result::Result<(), String> {
    let model = state.app.upstream.models(false).await.resolve(&model)?.0;
    state.start().await.map_err(|e| e.to_string())?;
    let (sender, receiver) = oneshot::channel();
    {
        let mut active = state.cancellations.lock().unwrap();
        if !active.is_empty() {
            return Err("已有对话正在生成，请先停止或等待完成。".into());
        }
        active.insert(request_id.clone(), sender);
    }
    let result = tokio::select! {
        _ = receiver => { let _ = on_event.send(ChatEvent::Cancelled); Ok(()) },
        result = receive_chat(&state, &model, thinking_effort, messages, &on_event) => result,
    };
    state.cancellations.lock().unwrap().remove(&request_id);
    if let Err(error) = result {
        let _ = on_event.send(ChatEvent::Error {
            message: error.to_string(),
        });
    }
    Ok(())
}

#[tauri::command]
fn cancel_chat(state: State<'_, Desktop>, request_id: String) {
    if let Some(sender) = state.cancellations.lock().unwrap().remove(&request_id) {
        let _ = sender.send(());
    }
}

fn load_key(directory: &Path) -> Result<String> {
    let path = directory.join("proxy.key");
    match std::fs::read_to_string(&path) {
        Ok(value) if value.trim().len() >= 16 => Ok(value.trim().to_owned()),
        Ok(_) => Err(anyhow!("本地代理密钥文件无效。")),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            let key = format!(
                "qw_{}{}",
                uuid::Uuid::new_v4().simple(),
                uuid::Uuid::new_v4().simple()
            );
            quickway::local_files::write_private(&path, key.as_bytes())?;
            Ok(key)
        }
        Err(error) => Err(error.into()),
    }
}

fn show_main(app: &tauri::AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
    }
}

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _, _| {
            show_main(app);
        }))
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_clipboard_manager::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .setup(|app| {
            let config_dir = app.path().app_config_dir()?;
            std::fs::create_dir_all(&config_dir)?;
            let preferences: Preferences = std::fs::read(config_dir.join("settings.json"))
                .ok()
                .and_then(|bytes| serde_json::from_slice(&bytes).ok())
                .unwrap_or_default();
            let auto_start = preferences.auto_start;
            let upstream = Upstream::new(auth::quick_base()?)?
                .with_model_cache(config_dir.join("models-cache.json"))
                .with_credential_file(config_dir.join("imported-credentials.qwc"));
            let key = load_key(&config_dir)?;
            app.manage(Desktop {
                config_dir,
                preferences: Mutex::new(preferences),
                app: Arc::new(server::App { upstream, key }),
                running: AsyncMutex::new(None),
                cancellations: Mutex::new(HashMap::new()),
                client: reqwest::Client::builder()
                    .no_proxy()
                    .redirect(reqwest::redirect::Policy::none())
                    .connect_timeout(Duration::from_secs(5))
                    .read_timeout(Duration::from_secs(150))
                    .build()?,
            });
            let show = MenuItem::with_id(app, "show", "打开 QuickWay", true, None::<&str>)?;
            let quit = MenuItem::with_id(app, "quit", "退出 QuickWay", true, None::<&str>)?;
            let menu = Menu::with_items(app, &[&show, &quit])?;
            app.manage(TrayMenu { show, quit });
            let mut tray = TrayIconBuilder::with_id(TRAY_ID)
                .menu(&menu)
                .tooltip("QuickWay · Quick 模型连接器")
                .show_menu_on_left_click(cfg!(target_os = "macos"))
                .on_menu_event(|app, event| match event.id.as_ref() {
                    "show" => {
                        show_main(app);
                    }
                    "quit" => {
                        let handle = app.clone();
                        tauri::async_runtime::spawn(async move {
                            handle.state::<Desktop>().stop().await;
                            handle.exit(0);
                        });
                    }
                    _ => {}
                })
                .on_tray_icon_event(|tray, event| {
                    if matches!(
                        event,
                        TrayIconEvent::Click {
                            button: MouseButton::Left,
                            button_state: MouseButtonState::Up,
                            ..
                        }
                    ) && let Some(window) = tray.app_handle().get_webview_window("main")
                    {
                        let _ = window.show();
                        let _ = window.set_focus();
                    }
                });
            #[cfg(target_os = "macos")]
            {
                tray = tray
                    .icon(tauri::include_image!("icons/tray.png"))
                    .icon_as_template(true);
            }
            #[cfg(not(target_os = "macos"))]
            if let Some(icon) = app.default_window_icon() {
                tray = tray.icon(icon.clone());
            }
            tray.build(app)?;
            if auto_start {
                let handle = app.handle().clone();
                tauri::async_runtime::spawn(async move {
                    let _ = handle.state::<Desktop>().start().await;
                });
            }
            Ok(())
        })
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                let _ = window.hide();
            }
        })
        .invoke_handler(tauri::generate_handler![
            get_status,
            start_proxy,
            stop_proxy,
            save_preferences,
            refresh_usage,
            diagnostics,
            refresh_models,
            authorize_quick,
            export_credentials,
            import_credentials,
            forget_credentials,
            open_quick,
            copy_connection,
            copy_text,
            set_tray_language,
            open_external,
            check_update,
            install_update,
            import_cc_switch,
            api_key,
            launch_claude,
            launch_codex,
            copy_codex_config,
            chat,
            cancel_chat
        ])
        .build(tauri::generate_context!())
        .expect("QuickWay could not start")
        .run(|app, event| {
            #[cfg(target_os = "macos")]
            if matches!(event, tauri::RunEvent::Reopen { .. }) {
                show_main(app);
            }
            #[cfg(not(target_os = "macos"))]
            let _ = (app, event);
        });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_installed_copies_update_themselves() {
        let dir = std::env::temp_dir().join(format!("quickway-update-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let exe = dir.join("quickway-desktop.exe");
        assert!(!replaceable_install(&exe, true), "portable exe has no uninstaller");
        std::fs::write(dir.join("uninstall.exe"), b"").unwrap();
        assert!(replaceable_install(&exe, true));
        std::fs::remove_dir_all(&dir).unwrap();

        let app = "QuickWay.app/Contents/MacOS/quickway-desktop";
        assert!(replaceable_install(Path::new(&format!("/Applications/{app}")), false));
        assert!(!replaceable_install(Path::new(&format!("/Volumes/QuickWay/{app}")), false));
        assert!(!replaceable_install(
            Path::new(&format!("/private/var/folders/x/AppTranslocation/1/d/{app}")),
            false
        ));
        assert!(!replaceable_install(Path::new("/usr/local/bin/quickway-desktop"), false));
    }

    #[test]
    fn chat_model_uses_served_metadata_and_never_the_requested_or_resolved_label() {
        let event = json!({"type":"message_delta", "model":"quick-smart", "quickway":{
            "resolved_model":"us.anthropic.claude-opus-5-5",
            "served_model":"us.anthropic.claude-opus-4-8"
        }});
        assert_eq!(
            serde_json::to_value(chat_model_event(&event).unwrap()).unwrap(),
            json!({"type":"model", "served":"us.anthropic.claude-opus-4-8"})
        );
        for event in [
            json!({"model":"claude-opus-5-5"}),
            json!({"quickway":{"resolved_model":"us.anthropic.claude-opus-5-5"}}),
            json!({"quickway":{"served_model":""}}),
            json!({"quickway":{"served_model":"https://invalid/token"}}),
            json!({"quickway":{"served_model":"<img src=x>"}}),
        ] {
            assert!(chat_model_event(&event).is_none());
        }
    }

    #[test]
    fn fresh_preferences_use_quick_smart_for_both_launchers() {
        let preferences: Preferences = serde_json::from_value(json!({})).unwrap();
        assert_eq!(preferences.model, "quick-smart");
        assert_eq!(preferences.codex_model(), "quick-smart");
        assert!(!quickway::models::is_gpt(preferences.codex_model()));
        let connection = claude_connection(&preferences, "synthetic-test-key");
        for field in [
            "ANTHROPIC_DEFAULT_OPUS_MODEL",
            "ANTHROPIC_DEFAULT_SONNET_MODEL",
            "ANTHROPIC_DEFAULT_HAIKU_MODEL",
            "ANTHROPIC_SMALL_FAST_MODEL",
        ] {
            assert_eq!(connection["env"][field], "quick-smart");
        }
        assert_eq!(codex_args(&preferences, None)[..2], ["-m", "quick-smart"]);
    }

    #[test]
    fn existing_preferences_keep_model_project_and_default_to_off() {
        let preferences: Preferences = serde_json::from_value(json!({"port":8741,"model":"claude-sonnet-5-5","projectDir":"C:/项目","autoStart":true})).unwrap();
        assert_eq!(preferences.thinking_effort, Effort::Off);
        assert_eq!(preferences.model, "claude-sonnet-5-5");
        assert_eq!(preferences.project_dir, "C:/项目");
    }

    #[test]
    fn cc_switch_link_carries_the_local_endpoint_key_and_models() {
        let preferences = Preferences {
            port: 9001,
            model: "gpt-6-astra".into(),
            ..Preferences::default()
        };
        let link = reqwest::Url::parse(&cc_switch_link(&preferences, "qw key&x=1")).unwrap();
        assert_eq!(
            (link.scheme(), link.host_str(), link.path()),
            ("ccswitch", Some("v1"), "/import")
        );
        let query: HashMap<String, String> = link.query_pairs().into_owned().collect();
        let env = &claude_connection(&preferences, "qw key&x=1")["env"];
        assert_eq!(query["resource"], "provider");
        assert_eq!(query["app"], "claude");
        assert_eq!(query["name"], "QuickWay");
        assert_eq!(query["endpoint"], "http://127.0.0.1:9001");
        // Reserved characters in the key survive the round trip instead of splitting the query.
        assert_eq!(query["apiKey"], "qw key&x=1");
        assert_eq!(query["model"], "gpt-6-astra");
        assert_eq!(query["opusModel"], env["ANTHROPIC_DEFAULT_OPUS_MODEL"]);
        assert_eq!(query["sonnetModel"], env["ANTHROPIC_DEFAULT_SONNET_MODEL"]);
        assert_eq!(query["haikuModel"], env["ANTHROPIC_DEFAULT_HAIKU_MODEL"]);
        for role in ["opusModel", "sonnetModel", "haikuModel"] {
            assert_eq!(query[role], preferences.model);
        }
        assert_eq!(env["ANTHROPIC_SMALL_FAST_MODEL"], preferences.model);
        assert!(!query.contains_key("x"));
    }

    #[test]
    fn claude_cloud_tiers_declare_a_client_budget_without_impersonating_a_model() {
        for tier in [
            "quick-fast",
            "quick-balanced",
            "quick-smart",
            "quick-advanced",
        ] {
            let preferences = Preferences {
                model: tier.into(),
                ..Preferences::default()
            };
            let config = claude_connection(&preferences, "synthetic-test-key");
            assert_eq!(config["env"]["ANTHROPIC_CUSTOM_MODEL_OPTION"], tier);
            assert_eq!(config["env"]["CLAUDE_CODE_MAX_CONTEXT_TOKENS"], "200000");
            assert!(config.get("modelOverrides").is_none());
        }
    }

    #[test]
    fn claude_connection_keeps_selected_effort_per_launch() {
        for effort in [Effort::Off, Effort::Low, Effort::Medium, Effort::High] {
            let preferences = Preferences {
                thinking_effort: effort,
                ..Preferences::default()
            };
            let config = claude_connection(&preferences, "synthetic-test-key");
            assert_eq!(config["alwaysThinkingEnabled"], effort != Effort::Off);
            assert_eq!(
                config["env"]["ANTHROPIC_CUSTOM_HEADERS"],
                format!("x-quickway-thinking-effort: {}", effort.as_str())
            );
            assert_eq!(
                config["env"]["MAX_THINKING_TOKENS"],
                effort.budget().to_string()
            );
            let restored: Preferences =
                serde_json::from_value(serde_json::to_value(preferences).unwrap()).unwrap();
            assert_eq!(restored.thinking_effort, effort);
        }
    }

    #[test]
    fn codex_args_point_at_quickway_without_double_quotes() {
        let preferences = Preferences {
            port: 9001,
            model: "claude-sonnet-5-5".into(),
            codex_effort: Effort::Off,
            ..Preferences::default()
        };
        // An empty Codex model follows the shared default.
        let args = codex_args(
            &preferences,
            Some(Path::new("C:/Users/中文 用户/codex-models.json")),
        );
        assert_eq!(args[..2], ["-m", "claude-sonnet-5-5"]);
        assert!(args.iter().all(|a| !a.contains(['"', '\n'])));
        for expected in [
            "model_provider='quickway'",
            "model_providers.quickway.base_url='http://127.0.0.1:9001/v1'",
            "model_providers.quickway.env_key='QUICKWAY_API_KEY'",
            "model_providers.quickway.http_headers.x-quickway-thinking-effort='off'",
            "model_reasoning_effort='none'",
            "model_catalog_json='C:/Users/中文 用户/codex-models.json'",
        ] {
            assert!(args.iter().any(|a| a == expected), "missing {expected}");
        }
        let gpt = Preferences {
            codex_model: "gpt-5.6-sol".into(),
            codex_effort: Effort::High,
            ..preferences
        };
        // A path that cannot be a TOML literal string drops the catalog instead of breaking quoting.
        let args = codex_args(&gpt, Some(Path::new("C:/it's/codex-models.json")));
        assert_eq!(args[1], "gpt-5.6-sol");
        assert!(args.iter().any(|a| a == "model_reasoning_effort='high'"));
        assert!(!args.iter().any(|a| a.starts_with("model_catalog_json")));
        assert!(!codex_config(&gpt).contains("qw_"));
    }

    #[test]
    fn codex_catalog_adds_missing_models_from_the_bundled_template() {
        let bundled = json!({"models":[
            {"slug":"gpt-hidden","visibility":"hide","display_name":"Hidden","base_instructions":"x"},
            {"slug":"gpt-5.6-sol","visibility":"list","display_name":"GPT-5.6 Sol","context_window":272000,
             "apply_patch_tool_type":"freeform","upgrade":{"model":"gpt-6"},
             "base_instructions":"You are Codex, an agent based on GPT-6. Work well."}]});
        let model = |id: &str, name: &str| quickway::models::Model {
            id: id.into(),
            upstream: String::new(),
            name: name.into(),
            family: String::new(),
        };
        let catalog = codex_catalog(
            &bundled,
            &[
                model("gpt-5.6-sol", "GPT-5.6 Sol"),
                model("claude-sonnet-5-5", "Claude Sonnet 5.5"),
                model("quick-smart", "Quick Smart (cloud)"),
            ],
        )
        .unwrap();
        let models = catalog["models"].as_array().unwrap();
        assert_eq!(models.len(), 4);
        assert_eq!(models[1], bundled["models"][1]);
        let claude = &models[2];
        assert_eq!(claude["slug"], "claude-sonnet-5-5");
        assert_eq!(claude["apply_patch_tool_type"], "freeform");
        assert_eq!(claude["context_window"], 200_000);
        assert!(claude.get("upgrade").is_none());
        assert_eq!(
            claude["base_instructions"],
            "You are Codex, an agent based on Claude Sonnet 5.5. Work well."
        );
        let quick = &models[3];
        assert_eq!(quick["slug"], "quick-smart");
        assert_eq!(quick["apply_patch_tool_type"], "freeform");
        assert_eq!(quick["visibility"], "list");
        assert!(codex_catalog(&json!({}), &[]).is_none());
    }

    /// Runs the real Windows launch command (PowerShell argument passing, the
    /// catalog built from the installed Codex) with `codex exec` against a
    /// running proxy: `QUICKWAY_TEST_PORT=8741 QUICKWAY_TEST_KEY=... cargo test -- --ignored`.
    #[cfg(windows)]
    #[test]
    #[ignore = "needs Codex, a running QuickWay proxy and Quick sign-in"]
    fn live_codex_launch_command_writes_and_runs_code() {
        let port: u16 = std::env::var("QUICKWAY_TEST_PORT")
            .unwrap()
            .parse()
            .unwrap();
        let key = std::env::var("QUICKWAY_TEST_KEY").unwrap();
        let model = std::env::var("QUICKWAY_TEST_MODEL").unwrap_or("claude-sonnet-5-5".into());
        let executable = platform::find_codex().expect("Codex is installed");
        let root =
            std::env::temp_dir().join(format!("QuickWay Codex 中文 {}", uuid::Uuid::new_v4()));
        let project = root.join("项目 one");
        std::fs::create_dir_all(&project).unwrap();
        let bundled = platform::codex_bundled_models(&executable).expect("bundled catalog");
        let models = [quickway::models::Model {
            id: model.clone(),
            upstream: String::new(),
            name: model.clone(),
            family: String::new(),
        }];
        let catalog_file = root.join("codex-models.json");
        std::fs::write(
            &catalog_file,
            serde_json::to_vec(&codex_catalog(&bundled, &models).unwrap()).unwrap(),
        )
        .unwrap();
        let preferences = Preferences {
            port,
            codex_model: model,
            codex_effort: Effort::Low,
            ..Preferences::default()
        };
        // `exec` must come first; root-level options before it start the interactive UI.
        let mut args: Vec<String> = ["exec", "--skip-git-repo-check", "-s", "danger-full-access"]
            .map(String::from)
            .into();
        args.extend(codex_args(&preferences, Some(&catalog_file)));
        args.push("Create add.py defining add(a, b) and printing add(2, 3). Run it with python and report the output.".into());
        let output = platform::codex_exec_for_test(&executable, &project, &args, &key);
        let log = format!(
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(output.status.success(), "{log}");
        assert!(
            !log.contains("Model metadata for"),
            "catalog was not used:\n{log}"
        );
        assert!(project.join("add.py").is_file(), "{log}");
        let run = std::process::Command::new("python")
            .arg("add.py")
            .current_dir(&project)
            .output()
            .unwrap();
        assert_eq!(String::from_utf8_lossy(&run.stdout).trim(), "5");
        std::fs::remove_dir_all(root).unwrap();
    }
}
