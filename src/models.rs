use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    future::Future,
    io::Read,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};
use tokio::sync::Mutex;

pub fn is_gpt(model_id: &str) -> bool {
    model_id.contains("openai.gpt-") || model_id.starts_with("gpt-")
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct Model {
    pub id: String,
    pub upstream: String,
    pub name: String,
    pub family: String,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Catalog {
    pub models: Vec<Model>,
    pub source: &'static str,
    pub last_updated: Option<u64>,
    pub error: Option<String>,
}

impl Catalog {
    pub fn resolve(&self, name: &str) -> Result<(String, String), String> {
        // Resolve only identifiers actually published by the current directory.
        if let Some(model) = self
            .models
            .iter()
            .find(|m| m.id == name || m.upstream == name)
        {
            return Ok((model.id.clone(), model.upstream.clone()));
        }
        Err(format!(
            "Model '{name}' is not in the Quick model directory. Refresh the directory or query /v1/models."
        ))
    }

    pub fn list(&self) -> Value {
        json!({"object":"list","data": self.models.iter().map(|m|json!({"id":m.id,"type":"model","display_name":m.name,"created_at":"2026-11-10T00:00:00Z","object":"model","created":1794268800u64,"owned_by":"quickway"})).collect::<Vec<_>>(),
            "has_more":false,"first_id":self.models.first().map(|m| &m.id),"last_id":self.models.last().map(|m| &m.id),
            "quickway_catalog":{"source":self.source,"last_updated":self.last_updated,"error":self.error}})
    }
}

impl Default for Catalog {
    fn default() -> Self {
        Self {
            models: Vec::new(),
            source: "unavailable",
            last_updated: None,
            error: None,
        }
    }
}

fn decode(value: &Value) -> Result<Value, String> {
    match value.as_str() {
        Some(text) => serde_json::from_str(text).map_err(|_| "模型目录格式无效。".into()),
        None => Ok(value.clone()),
    }
}

pub fn is_anthropic(model_id: &str) -> bool {
    unregionalized(model_id).starts_with("anthropic.")
}

fn unregionalized(route: &str) -> &str {
    ["global.", "us.", "eu.", "apac."]
        .iter()
        .find_map(|prefix| route.strip_prefix(prefix))
        .unwrap_or(route)
}

fn public_id(route: &str) -> Option<&str> {
    // Validate identifier syntax, never model families or protocol support.
    if route.is_empty()
        || route.len() > 512
        || !route
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"-._:/".contains(&b))
        || route.contains("://")
        || route.contains("..")
    {
        return None;
    }
    let route = unregionalized(route);
    if let Some(id) = route.strip_prefix("anthropic.") {
        return (!id.is_empty()).then_some(match id.rsplit_once("-v") {
            Some((base, version))
                if !version.is_empty()
                    && version.bytes().all(|b| b.is_ascii_digit() || b == b':') =>
            {
                base
            }
            _ => id,
        });
    }
    if let Some(id) = route.strip_prefix("openai.") {
        return (!id.is_empty()).then_some(id);
    }
    Some(route)
}

fn family(route: &str) -> String {
    match unregionalized(route)
        .split_once('.')
        .map(|(provider, _)| provider)
    {
        Some("anthropic") => "Anthropic".into(),
        Some("openai") => "OpenAI".into(),
        Some("amazon") => "Amazon".into(),
        Some(provider) => provider.into(),
        None => "其他".into(),
    }
}

pub(crate) fn parse_config(outer: &Value) -> Result<Vec<Model>, String> {
    let response = decode(&outer["eventResponse"]["invokeAgentResponse"])?;
    if response["status"] != 200 {
        return Err("Quick 未能提供模型目录，请稍后刷新。".into());
    }
    let body = decode(&response["body"])?;
    let rows = body["values"]["models"]
        .as_object()
        .ok_or("模型目录格式无效。")?;
    // Deduplicate repeated slots for the exact same route. Distinct routes must
    // remain visible even if their shortened display IDs would otherwise collide.
    let mut models = BTreeMap::<String, Model>::new();
    for (slot, row) in rows {
        let route = row["model_id"]
            .as_str()
            .ok_or_else(|| format!("模型目录条目 {slot} 缺少 model_id。"))?;
        let id = public_id(route)
            .ok_or_else(|| format!("模型目录条目 {slot} 的 model_id 格式无效。"))?;
        let name = row["display_name"]
            .as_str()
            .filter(|s| !s.trim().is_empty() && s.len() <= 512 && !s.chars().any(char::is_control))
            .unwrap_or(id);
        let model = Model {
            id: id.into(),
            upstream: route.into(),
            name: name.into(),
            family: family(route),
        };
        if models.get(route).is_none_or(|old| model.name < old.name) {
            models.insert(route.into(), model);
        }
    }
    let mut counts = BTreeMap::<String, usize>::new();
    for model in models.values() {
        *counts.entry(model.id.clone()).or_default() += 1;
    }
    // A shortened ID must also not shadow another model's exact route.
    let routes: std::collections::BTreeSet<_> = models.keys().cloned().collect();
    for model in models.values_mut() {
        if counts[&model.id] > 1 || (model.id != model.upstream && routes.contains(&model.id)) {
            model.id = model.upstream.clone();
        }
    }
    let mut result: Vec<_> = models.into_values().collect();
    result.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(result)
}

#[derive(Default)]
struct Cached {
    identity: Option<[u8; 32]>,
    checked: Option<Instant>,
    catalog: Catalog,
}

#[derive(Default)]
pub struct Directory {
    cached: Mutex<Cached>,
    cache_path: Option<PathBuf>,
}

// Store only validated model metadata, never credentials or upstream config.
#[derive(Deserialize, Serialize)]
struct SavedCatalog {
    version: u8,
    identity: [u8; 32],
    last_updated: u64,
    models: BTreeMap<String, SavedModel>,
}

#[derive(Deserialize, Serialize)]
struct SavedModel {
    model_id: String,
    display_name: String,
}

fn restore_catalog(path: &Path, identity: [u8; 32]) -> Option<Catalog> {
    let mut bytes = Vec::new();
    std::fs::File::open(path)
        .ok()?
        .take(2 * 1024 * 1024 + 1)
        .read_to_end(&mut bytes)
        .ok()?;
    if bytes.len() > 2 * 1024 * 1024 {
        return None;
    }
    let saved: SavedCatalog = serde_json::from_slice(&bytes).ok()?;
    // Version 1 filtered model families before saving; it is not a complete
    // cloud snapshot. Fetch again on upgrade instead of restoring that subset.
    if saved.version != 2 || saved.identity != identity || saved.last_updated == 0 {
        return None;
    }
    // Apply exactly the same identifier and display-name validation as a
    // live discovery response; do not trust a locally edited cache file.
    let models = parse_config(&json!({"eventResponse":{"invokeAgentResponse":{
        "status":200,"body":{"values":{"models":saved.models}}
    }}}))
    .ok()?;
    Some(Catalog {
        models,
        source: "cache",
        last_updated: Some(saved.last_updated),
        error: Some("正在使用上次同步的模型目录，连接恢复后会自动更新。".into()),
    })
}

fn save_catalog(path: &Path, identity: [u8; 32], catalog: &Catalog) -> std::io::Result<()> {
    let saved = SavedCatalog {
        version: 2,
        identity,
        last_updated: catalog.last_updated.unwrap_or_default(),
        models: catalog
            .models
            .iter()
            .map(|model| {
                (
                    model.id.clone(),
                    SavedModel {
                        model_id: model.upstream.clone(),
                        display_name: model.name.clone(),
                    },
                )
            })
            .collect(),
    };
    let bytes = serde_json::to_vec(&saved)?;
    let temporary = path.with_extension(format!("tmp-{}", uuid::Uuid::new_v4()));
    let result = crate::local_files::write_private(&temporary, &bytes)
        .and_then(|()| std::fs::rename(&temporary, path));
    if result.is_err() {
        let _ = std::fs::remove_file(temporary);
    }
    result
}

impl Directory {
    pub(crate) fn with_cache(path: PathBuf) -> Self {
        Self {
            cache_path: Some(path),
            ..Self::default()
        }
    }

    /// One fetch per profile at startup, then every five minutes on use. Failed
    /// fetches retry after a minute; explicit refresh bypasses that backoff.
    /// Profile changes clear the old directory. Credential failures preserve it.
    pub(crate) async fn get<F, Fut>(
        &self,
        identity: Option<[u8; 32]>,
        force: bool,
        fetch: F,
    ) -> Catalog
    where
        F: FnOnce() -> Fut,
        Fut: Future<Output = Result<Vec<Model>, String>>,
    {
        let started = Instant::now();
        let mut cached = self.cached.lock().await;
        if identity != cached.identity {
            let restored = identity.and_then(|identity| {
                self.cache_path
                    .as_deref()
                    .and_then(|path| restore_catalog(path, identity))
            });
            if restored.is_none()
                && let Some(path) = &self.cache_path
            {
                let _ = std::fs::remove_file(path);
            }
            *cached = Cached {
                identity,
                catalog: restored.unwrap_or_default(),
                ..Cached::default()
            };
        }
        if identity.is_none() {
            if let Some(path) = &self.cache_path {
                let _ = std::fs::remove_file(path);
            }
            cached.catalog.error = Some("请登录 Amazon Quick 后刷新模型目录。".into());
            return cached.catalog.clone();
        }
        let ttl = if cached.catalog.error.is_some() {
            60
        } else {
            300
        };
        if cached.checked.is_some_and(|checked| {
            checked >= started || (!force && checked.elapsed() < Duration::from_secs(ttl))
        }) {
            return cached.catalog.clone();
        }
        match fetch().await {
            Ok(models) => {
                cached.catalog = Catalog {
                    models,
                    source: "cloud",
                    last_updated: Some(crate::auth::now() as u64),
                    error: None,
                };
                if let (Some(path), Some(identity)) = (&self.cache_path, identity)
                    && save_catalog(path, identity, &cached.catalog).is_err()
                {
                    tracing::warn!(
                        "Could not persist model metadata; keeping the in-memory directory"
                    );
                }
            }
            Err(error) => {
                cached.catalog.source = if cached.catalog.last_updated.is_some() {
                    "cache"
                } else {
                    "unavailable"
                };
                cached.catalog.error = Some(error);
            }
        }
        cached.checked = Some(Instant::now());
        cached.catalog.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn config(rows: Value, encoded: bool) -> Value {
        let body = json!({"values":{"models":rows}, "unrelated":{"model_id":"us.openai.gpt-fake"}});
        let response =
            json!({"status":200,"body":if encoded {json!(body.to_string())} else {body}});
        json!({"eventResponse":{"invokeAgentResponse":if encoded {json!(response.to_string())} else {response}}})
    }
    fn discovered(id: &str) -> Vec<Model> {
        parse_config(&config(
            json!({"slot":{"model_id":format!("us.anthropic.{id}"),"display_name":"New Claude"}}),
            false,
        ))
        .unwrap()
    }

    #[test]
    fn publishes_all_cloud_families_without_adding_models() {
        let rows = json!({
            "a":{"model_id":"us.anthropic.claude-fable-5-1","display_name":"Fable 5.1"},
            "thinking":{"model_id":"us.anthropic.claude-fable-5-1","display_name":"Fable 5.1"},
            "nova":{"model_id":"amazon.nova-2-5-sonic","display_name":"Nova"},
            "future":{"model_id":"us.newvendor.future-1","display_name":"Future"},
            "gpt":{"model_id":"eu.openai.gpt-99-new","display_name":"Future GPT"},
            "version":{"model_id":"us.anthropic.claude-haiku-4-5-20251001-v1:0"}
        });
        let plain = parse_config(&config(rows.clone(), false)).unwrap();
        assert_eq!(plain, parse_config(&config(rows, true)).unwrap());
        assert_eq!(plain.len(), 5);
        let catalog = Catalog {
            models: plain,
            ..Catalog::default()
        };
        assert_eq!(
            catalog.resolve("claude-fable-5-1").unwrap().1,
            "us.anthropic.claude-fable-5-1"
        );
        assert_eq!(
            catalog.resolve("amazon.nova-2-5-sonic").unwrap().1,
            "amazon.nova-2-5-sonic"
        );
        assert_eq!(
            catalog.resolve("newvendor.future-1").unwrap().1,
            "us.newvendor.future-1"
        );
        assert!(catalog.resolve("gpt-99-new").is_ok());
        assert!(catalog.resolve("claude-haiku-4-5-20251001").is_ok());
        for absent in [
            "opus",
            "sonnet",
            "haiku",
            "claude-opus-4-6",
            "claude-sonnet-4-6",
        ] {
            assert!(catalog.resolve(absent).is_err());
        }
        assert_eq!(catalog.list()["data"].as_array().unwrap().len(), 5);
    }

    #[test]
    fn preserves_distinct_routes_with_colliding_short_ids() {
        let models = parse_config(&config(
            json!({
                "a":{"model_id":"us.anthropic.claude-fable-5-1"},
                "b":{"model_id":"global.anthropic.claude-fable-5-1"},
                "c":{"model_id":"us.anthropic.claude-fable-5-1"}
            }),
            false,
        ))
        .unwrap();
        let catalog = Catalog {
            models,
            ..Catalog::default()
        };
        assert_eq!(catalog.models.len(), 2);
        for route in [
            "us.anthropic.claude-fable-5-1",
            "global.anthropic.claude-fable-5-1",
        ] {
            assert_eq!(
                catalog.resolve(route).unwrap(),
                (route.into(), route.into())
            );
        }
        assert!(catalog.resolve("claude-fable-5-1").is_err());
    }

    #[test]
    fn accepts_empty_cloud_directory_and_reports_invalid_entries() {
        assert!(parse_config(&config(json!({}), false)).unwrap().is_empty());
        for value in [
            json!({}),
            json!({"eventResponse":{"invokeAgentResponse":"bad"}}),
            json!({"eventResponse":{"invokeAgentResponse":{"status":403,"body":{}}}}),
            config(json!([]), false),
            config(json!({"slot":{}}), false),
        ] {
            assert!(parse_config(&value).is_err());
        }
        for route in [
            "",
            "https://attacker.invalid/model",
            "us.openai.gpt-new\n",
            "us.anthropic.claude-opus-5/../../",
        ] {
            // Invalid entries fail the refresh instead of silently publishing a partial list.
            assert!(
                parse_config(&config(
                    json!({
                        "good":{"model_id":"us.anthropic.claude-fable-5-1"},
                        "bad":{"model_id":route}
                    }),
                    false
                ))
                .is_err()
            );
        }
        let models = parse_config(&config(
            json!({"a":{"model_id":"us.anthropic.claude-fable-5-1","display_name":"bad\nname"}}),
            false,
        ))
        .unwrap();
        assert_eq!(models[0].name, "claude-fable-5-1");
    }

    #[tokio::test]
    async fn first_fetch_failure_has_no_models_and_successful_empty_refresh_clears_disk_cache() {
        let path =
            std::env::temp_dir().join(format!("quickway-catalog-{}.json", uuid::Uuid::new_v4()));
        let key = Some([2; 32]);
        let directory = Directory::with_cache(path.clone());
        let initial = directory
            .get(key, false, || async { Err("offline".into()) })
            .await;
        assert_eq!(initial.source, "unavailable");
        assert!(initial.models.is_empty());
        directory
            .get(key, true, || async { Ok(discovered("claude-fable-5-1")) })
            .await;
        let empty = directory.get(key, true, || async { Ok(Vec::new()) }).await;
        assert_eq!(empty.source, "cloud");
        assert!(empty.error.is_none());
        assert!(empty.models.is_empty());
        assert!(empty.resolve("claude-fable-5-1").is_err());
        let restarted = Directory::with_cache(path.clone());
        let restored = restarted
            .get(key, false, || async { Err("offline".into()) })
            .await;
        assert_eq!(restored.source, "cache");
        assert!(restored.models.is_empty());
        std::fs::remove_file(path).unwrap();
    }

    #[tokio::test]
    async fn caches_success_retries_failure_and_replaces_the_entire_directory() {
        let directory = Directory::default();
        let key = Some([1; 32]);
        let first = directory
            .get(key, false, || async { Ok(discovered("claude-opus-9")) })
            .await;
        assert_eq!(first.source, "cloud");
        let cached = directory
            .get(key, false, || async {
                panic!("fresh cache must not fetch")
            })
            .await;
        assert_eq!(cached.models, first.models);
        let failed = directory
            .get(key, true, || async { Err("offline fixture".into()) })
            .await;
        assert_eq!(failed.source, "cache");
        assert_eq!(failed.models, first.models);
        assert_eq!(failed.last_updated, first.last_updated);
        directory
            .get(key, false, || async {
                panic!("failed refresh must back off")
            })
            .await;
        directory.cached.lock().await.checked = Some(Instant::now() - Duration::from_secs(61));
        let recovered = directory
            .get(key, false, || async { Ok(discovered("claude-sonnet-9")) })
            .await;
        assert!(recovered.error.is_none());
        assert!(recovered.resolve("claude-opus-9").is_err());
        assert!(recovered.resolve("claude-sonnet-9").is_ok());
        assert!(recovered.resolve("claude-opus-4-6").is_err());
        directory.cached.lock().await.checked = Some(Instant::now() - Duration::from_secs(301));
        let auto = directory
            .get(key, false, || async { Ok(discovered("claude-haiku-9")) })
            .await;
        assert!(auto.resolve("claude-haiku-9").is_ok());
        assert!(auto.resolve("claude-sonnet-9").is_err());
    }

    #[tokio::test]
    async fn login_change_and_signout_never_reuse_previous_cloud_directory() {
        let directory = Directory::default();
        directory
            .get(Some([1; 32]), false, || async {
                Ok(discovered("claude-opus-9"))
            })
            .await;
        let changed = directory
            .get(Some([2; 32]), false, || async { Err("offline".into()) })
            .await;
        assert_eq!(changed.source, "unavailable");
        assert!(changed.models.is_empty());
        assert!(changed.resolve("claude-opus-9").is_err());
        assert!(changed.last_updated.is_none());
        let signedout = directory
            .get(None, false, || async { panic!("no login") })
            .await;
        assert!(signedout.models.is_empty());
        assert!(signedout.error.is_some());
    }

    #[tokio::test]
    async fn keychain_denial_and_restart_preserve_discovered_models_for_the_same_profile() {
        let path =
            std::env::temp_dir().join(format!("quickway-catalog-{}.json", uuid::Uuid::new_v4()));
        let identity = Some([7; 32]);
        let directory = Directory::with_cache(path.clone());
        let first = directory
            .get(identity, false, || async {
                Ok(discovered("claude-opus-5-5"))
            })
            .await;
        directory.cached.lock().await.checked = Some(Instant::now() - Duration::from_secs(301));
        let denied = directory
            .get(identity, false, || async {
                Err("钥匙串尚未授权".into())
            })
            .await;
        assert_eq!(denied.source, "cache");
        assert_eq!(denied.models, first.models);
        assert_eq!(denied.last_updated, first.last_updated);
        assert!(denied.error.unwrap().contains("钥匙串"));
        drop(directory);
        let restarted = Directory::with_cache(path.clone());
        let offline = restarted
            .get(identity, false, || async {
                Err("钥匙串尚未授权".into())
            })
            .await;
        assert_eq!(offline.source, "cache");
        assert_eq!(offline.models, first.models);
        assert_eq!(offline.last_updated, first.last_updated);
        assert!(offline.resolve("claude-opus-5-5").is_ok());
        let saved: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        assert_eq!(saved["version"], 2);
        let keys: Vec<_> = saved
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect();
        assert_eq!(keys, ["identity", "last_updated", "models", "version"]);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
        let changed = restarted
            .get(Some([8; 32]), false, || async {
                Err("other profile is offline".into())
            })
            .await;
        assert!(changed.resolve("claude-opus-5-5").is_err());
        assert!(!path.exists());
        restarted
            .get(Some([8; 32]), true, || async {
                Ok(discovered("claude-haiku-5-5"))
            })
            .await;
        assert!(path.exists());
        restarted
            .get(None, false, || async { panic!("no active profile") })
            .await;
        assert!(!path.exists());
    }

    #[tokio::test]
    async fn invalid_disk_catalog_never_introduces_unvalidated_model_routes() {
        let path =
            std::env::temp_dir().join(format!("quickway-catalog-{}.json", uuid::Uuid::new_v4()));
        let key = [4; 32];
        for contents in [
            b"not json".to_vec(),
            serde_json::to_vec(&json!({"version":2,"identity":key,"last_updated":1,"models":{
                "injected":{"model_id":"https://attacker.invalid/route","display_name":"Injected"}
            }})).unwrap(),
            serde_json::to_vec(&json!({"version":1,"identity":key,"last_updated":1,"models":{
                "previously-filtered":{"model_id":"us.anthropic.claude-opus-5-5","display_name":"Old partial directory"}
            }})).unwrap(),
            serde_json::to_vec(&json!({"version":99,"identity":key,"last_updated":1,"models":{}})).unwrap(),
        ] {
            std::fs::write(&path, contents).unwrap();
            let directory = Directory::with_cache(path.clone());
            let catalog = directory.get(Some(key), false, || async { Err("offline".into()) }).await;
            assert_eq!(catalog.source, "unavailable");
            assert!(catalog.models.is_empty());
            assert!(!path.exists());
        }
    }

    #[tokio::test]
    async fn concurrent_automatic_and_manual_refreshes_share_one_fetch() {
        let directory = Directory::default();
        let calls = AtomicUsize::new(0);
        let fetch = || async {
            calls.fetch_add(1, Ordering::SeqCst);
            tokio::task::yield_now().await;
            Ok(discovered("claude-opus-9"))
        };
        let (a, b, c) = tokio::join!(
            directory.get(Some([1; 32]), false, fetch),
            directory.get(Some([1; 32]), true, fetch),
            directory.get(Some([1; 32]), false, fetch)
        );
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert_eq!(a.models, b.models);
        assert_eq!(b.models, c.models);
    }
}
