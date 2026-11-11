//! Reuse local Quick login in memory, or explicitly imported credentials.
use crate::{
    auth::{self, QuickSession},
    portable::{self, PortableSession},
};
use anyhow::{Result, anyhow, bail};
use reqwest::Client;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::{
    path::{Path, PathBuf},
    time::Duration,
};
use tokio::sync::Mutex;

const TOKEN_ENDPOINT: &str = "https://oidc.us-east-1.amazonaws.com/token";

// Secret-bearing types deliberately do not implement Debug.
struct CachedSession {
    disk_fingerprint: [u8; 32],
    profile_identity: Option<[u8; 32]>,
    session: QuickSession,
}

pub struct SessionManager {
    base: PathBuf,
    cached: Mutex<Option<CachedSession>>,
    credential_path: Option<PathBuf>,
    source: Mutex<Source>,
}

enum Source {
    Local,
    Unavailable,
    Imported(Box<PortableSession>),
}

impl SessionManager {
    pub fn new(base: PathBuf) -> Self {
        Self {
            base,
            cached: Mutex::new(None),
            credential_path: None,
            source: Mutex::new(Source::Local),
        }
    }

    pub fn with_credential_file(mut self, path: PathBuf) -> Self {
        if path.try_exists().unwrap_or(true) {
            self.source = Mutex::new(
                match portable::read(&path).and_then(|bytes| PortableSession::decode(&bytes)) {
                    Ok(imported) => Source::Imported(Box::new(imported)),
                    Err(_) => Source::Unavailable,
                },
            );
        }
        self.credential_path = Some(path);
        self
    }

    pub fn quick_base(&self) -> &Path {
        &self.base
    }

    /// Non-secret source state; an unreadable/expired import never falls back to another account.
    pub async fn credential_source(&self) -> &'static str {
        match &*self.source.lock().await {
            Source::Local => "local",
            Source::Unavailable | Source::Imported(_) => "imported",
        }
    }

    pub async fn session(&self, client: &Client) -> Result<QuickSession> {
        let mut source = self.source.lock().await;
        match &mut *source {
            Source::Unavailable => bail!("导入凭证文件无法读取，请在「连接设置」重新导入或移除。"),
            Source::Imported(imported) => {
                return self
                    .imported_session(imported, client, TOKEN_ENDPOINT)
                    .await;
            }
            Source::Local => {}
        }
        #[cfg(target_os = "macos")]
        {
            self.profile_session(client, false).await
        }
        #[cfg(not(target_os = "macos"))]
        {
            self.session_with(client, || auth::load_session(&self.base), TOKEN_ENDPOINT)
                .await
        }
    }

    /// Import the explicitly authorized session into the same cache used by
    /// chat, model discovery and account status. Never discard this read.
    pub async fn authorize(&self, client: &Client) -> Result<QuickSession> {
        let source = self.source.lock().await;
        if !matches!(*source, Source::Local) {
            bail!("当前使用导入凭证。如需授权本机 Quick，请先移除导入凭证。");
        }
        self.profile_session(client, true).await
    }

    async fn imported_session(
        &self,
        imported: &mut PortableSession,
        client: &Client,
        endpoint: &str,
    ) -> Result<QuickSession> {
        if imported.rejected {
            bail!("导入凭证已被服务端拒绝，请在原机器续期后重新导出并导入。");
        }
        if !token_valid(&imported.session) && imported.session.managed_by_quick {
            bail!("导入的企业凭证已过期，请在原机器打开 Quick 续期后重新导出并导入。");
        }
        let renewed = usable_session(&imported.session, client, endpoint).await?;
        if disk_fingerprint(&renewed) != disk_fingerprint(&imported.session) {
            // Keep the rotated token in memory even if persistence fails; never retry with its predecessor.
            imported.session = renewed;
            imported.dirty = true;
        }
        // Retry a failed save before permitting further use, including when the new token is still valid.
        if imported.dirty {
            portable::save(
                self.credential_path
                    .as_deref()
                    .ok_or_else(|| anyhow!("未配置凭证保存位置。"))?,
                imported,
            )?;
            imported.dirty = false;
        }
        Ok(imported.session.clone())
    }

    pub async fn import_credentials(&self, path: PathBuf) -> Result<()> {
        let imported =
            tokio::task::spawn_blocking(move || PortableSession::decode(&portable::read(&path)?))
                .await
                .map_err(|_| anyhow!("读取凭证未完成。"))??;
        let mut source = self.source.lock().await;
        self.activate_import(imported, &mut source).await
    }

    async fn activate_import(&self, imported: PortableSession, source: &mut Source) -> Result<()> {
        let path = self
            .credential_path
            .as_deref()
            .ok_or_else(|| anyhow!("未配置凭证保存位置。"))?;
        if !token_valid(&imported.session) {
            if imported.session.managed_by_quick {
                bail!("企业凭证已过期，请在原机器续期后重新导出。");
            }
            validate_refresh(&imported.session)?;
        }
        // Persist and activate before attempting any token rotation. Subsequent use
        // goes through imported_session, which retains rotated tokens if a save fails.
        portable::save(path, &imported)?;
        *source = Source::Imported(Box::new(imported));
        *self.cached.lock().await = None;
        Ok(())
    }

    pub async fn forget_credentials(&self) -> Result<()> {
        let mut source = self.source.lock().await;
        if let Some(path) = &self.credential_path {
            match std::fs::remove_file(path) {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(_) => bail!("无法移除导入凭证，请检查文件权限。"),
            }
        }
        *source = Source::Local;
        *self.cached.lock().await = None;
        Ok(())
    }

    pub async fn export_credentials(&self, path: &Path, client: &Client) -> Result<()> {
        let session = self.session(client).await?;
        let path = path.to_owned();
        tokio::task::spawn_blocking(move || portable::export(&path, session))
            .await
            .map_err(|_| anyhow!("导出凭证未完成。"))?
    }

    async fn profile_session(&self, client: &Client, authorize: bool) -> Result<QuickSession> {
        let base = self.base.clone();
        self.profile_session_with(
            client,
            authorize,
            || auth::profile_identity(&self.base),
            || async move {
                tokio::task::spawn_blocking(move || {
                    if authorize {
                        auth::authorize_session(&base)
                    } else {
                        auth::load_session(&base)
                    }
                })
                .await
                .map_err(|_| anyhow!("读取 Quick 登录未完成，请重试。"))?
            },
            TOKEN_ENDPOINT,
        )
        .await
    }

    async fn profile_session_with<F: std::future::Future<Output = Result<QuickSession>>>(
        &self,
        client: &Client,
        authorize: bool,
        identity: impl Fn() -> Result<[u8; 32]>,
        load: impl FnOnce() -> F,
        endpoint: &str,
    ) -> Result<QuickSession> {
        let mut cached = self.cached.lock().await;
        let profile = match identity() {
            Ok(profile) => profile,
            Err(error) => {
                *cached = None;
                return Err(error);
            }
        };
        if cached
            .as_ref()
            .is_some_and(|entry| entry.profile_identity != Some(profile))
        {
            *cached = None;
        }
        // Quick rewrites its Keychain items during renewal. A lost item ACL
        // does not revoke a token already authorized and still valid in memory.
        if !authorize
            && let Some(entry) = cached.as_ref().filter(|entry| token_valid(&entry.session))
        {
            return Ok(entry.session.clone());
        }
        let loaded = load().await;
        // A user can change accounts while a system authorization dialog is open.
        if identity().ok() != Some(profile) {
            *cached = None;
            bail!("Quick 登录已变更，请重新连接当前账号。");
        }
        let disk = match loaded {
            Ok(session) => session,
            Err(error) => {
                if cached
                    .as_ref()
                    .is_none_or(|entry| !token_valid(&entry.session))
                {
                    *cached = None;
                }
                return Err(error);
            }
        };
        let fingerprint = disk_fingerprint(&disk);
        let current = cached
            .as_ref()
            .filter(|entry| entry.disk_fingerprint == fingerprint)
            .map(|entry| &entry.session)
            .unwrap_or(&disk);
        let session = usable_session(current, client, endpoint).await?;
        *cached = Some(CachedSession {
            disk_fingerprint: fingerprint,
            profile_identity: Some(profile),
            session: session.clone(),
        });
        Ok(session)
    }

    /// A rejected request must not keep using its cached token. An older
    /// in-flight response must not clear a newer authorization, however.
    pub async fn reject(&self, rejected: &QuickSession) {
        let mut source = self.source.lock().await;
        if let Source::Imported(imported) = &mut *source {
            if disk_fingerprint(&imported.session) == disk_fingerprint(rejected) {
                imported.rejected = true;
            }
            return;
        }
        let mut cached = self.cached.lock().await;
        if cached
            .as_ref()
            .is_some_and(|entry| disk_fingerprint(&entry.session) == disk_fingerprint(rejected))
        {
            *cached = None;
        }
    }

    /// Model metadata follows the active profile, independently of token
    /// rotation. Requests check this identity before reusing an authorized token.
    pub(crate) async fn catalog_identity(&self) -> Result<[u8; 32]> {
        match &*self.source.lock().await {
            Source::Local => auth::profile_identity(&self.base),
            Source::Unavailable => bail!("导入凭证无法读取，请重新导入或移除。"),
            Source::Imported(imported) => Ok(imported.identity),
        }
    }

    #[cfg(any(not(target_os = "macos"), test))]
    async fn session_with(
        &self,
        client: &Client,
        load: impl FnOnce() -> Result<QuickSession>,
        endpoint: &str,
    ) -> Result<QuickSession> {
        // Read after acquiring the lock: queued calls see Quick's latest login,
        // and only one caller can rotate a refresh token at a time.
        let mut cached = self.cached.lock().await;
        let disk = match load() {
            Ok(session) => session,
            Err(error) => {
                *cached = None;
                return Err(error);
            }
        };
        let fingerprint = disk_fingerprint(&disk);
        if cached
            .as_ref()
            .is_some_and(|entry| entry.disk_fingerprint != fingerprint)
        {
            *cached = None;
        }
        let current = cached.as_ref().map(|entry| &entry.session).unwrap_or(&disk);
        let session = usable_session(current, client, endpoint).await?;
        *cached = Some(CachedSession {
            disk_fingerprint: fingerprint,
            profile_identity: None,
            session: session.clone(),
        });
        Ok(session)
    }
}

fn token_valid(session: &QuickSession) -> bool {
    !session.access_token.is_empty() && session.access_token_expiry > auth::now()
}

async fn usable_session(
    session: &QuickSession,
    client: &Client,
    endpoint: &str,
) -> Result<QuickSession> {
    if token_valid(session) {
        return Ok(session.clone());
    }
    if session.managed_by_quick {
        bail!("Quick 企业登录已过期，请打开 Amazon Quick 完成续期，然后重试。");
    }
    refresh(session, client, endpoint).await
}

fn disk_fingerprint(session: &QuickSession) -> [u8; 32] {
    let mut hash = Sha256::new();
    hash.update([u8::from(session.managed_by_quick)]);
    // Include identity and registration changes as well as access-token rotation.
    // Length prefixes keep adjacent field combinations unambiguous.
    for value in [
        &session.access_token,
        &session.tenant_url,
        &session.issuer_url,
        &session.aws_account_id,
        &session.client_id,
        &session.client_secret,
        &session.refresh_token,
    ] {
        hash.update((value.len() as u64).to_be_bytes());
        hash.update(value.as_bytes());
    }
    for expiry in [
        session.access_token_expiry,
        session.refresh_token_expiry,
        session.client_secret_expires_at,
    ] {
        hash.update(expiry.to_bits().to_be_bytes());
    }
    hash.finalize().into()
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RefreshResponse {
    access_token: String,
    expires_in: u64,
    refresh_token: Option<String>,
}

fn validate_refresh(current: &QuickSession) -> Result<()> {
    let now = auth::now();
    if current.refresh_token.is_empty() || current.refresh_token_expiry <= now {
        bail!("Quick refresh token has expired; sign in to Amazon Quick again");
    }
    if current.client_id.is_empty()
        || current.client_secret.is_empty()
        || current.client_secret_expires_at <= now
    {
        bail!("Quick client registration has expired; sign in to Amazon Quick again");
    }
    Ok(())
}

async fn refresh(current: &QuickSession, client: &Client, endpoint: &str) -> Result<QuickSession> {
    validate_refresh(current)?;
    let now = auth::now();
    // Do not retry this POST: a successful response lost in transit may already
    // have rotated the refresh token. Never write back to Quick's credential store.
    let response = client
        .post(endpoint)
        .timeout(Duration::from_secs(30))
        .json(&serde_json::json!({
            "grantType": "refresh_token",
            "clientId": current.client_id,
            "clientSecret": current.client_secret,
            "refreshToken": current.refresh_token,
        }))
        .send()
        .await
        .map_err(|_| {
            anyhow!("Quick token refresh could not reach AWS; check the network and try again")
        })?;
    if !response.status().is_success() {
        bail!(
            "Quick token refresh failed (HTTP {}); reopen Amazon Quick to renew its login",
            response.status().as_u16()
        );
    }
    // Drop parse details and upstream bodies: either can contain credential data.
    let token: RefreshResponse = response
        .json()
        .await
        .map_err(|_| anyhow!("Quick token refresh returned an invalid token response"))?;
    if token.access_token.is_empty()
        || token.expires_in == 0
        || token
            .refresh_token
            .as_ref()
            .is_some_and(|value| value.is_empty())
    {
        bail!("Quick token refresh returned an invalid token response");
    }
    let mut renewed = current.clone();
    renewed.access_token = token.access_token;
    // Anchor to request start so network delay cannot extend the server lifetime.
    renewed.access_token_expiry = now + token.expires_in as f64;
    if let Some(refresh_token) = token.refresh_token {
        renewed.refresh_token = refresh_token;
    }
    // AWS does not return a refresh-token lifetime here. Preserve the known
    // deadline rather than inventing a new one for a rotated token.
    Ok(renewed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{Json, Router, routing::post};
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };

    #[tokio::test]
    async fn portable_import_restart_rejection_and_removal_without_native_quick() {
        let dir = tempfile::tempdir().unwrap();
        let exported = dir.path().join("export.qwc");
        let saved = dir.path().join("imported.qwc");
        portable::export(&exported, portable::tests::fixture(true)).unwrap();
        let manager =
            SessionManager::new(dir.path().join("no-quick")).with_credential_file(saved.clone());
        let client = Client::new();
        manager.import_credentials(exported.clone()).await.unwrap();
        assert!(manager.session(&client).await.unwrap().managed_by_quick);
        let identity = manager.catalog_identity().await.unwrap();
        let original = std::fs::read(&saved).unwrap();
        let malformed = dir.path().join("malformed.qwc");
        std::fs::write(&malformed, b"{broken").unwrap();
        assert!(manager.import_credentials(malformed).await.is_err());
        assert_eq!(manager.catalog_identity().await.unwrap(), identity);
        assert_eq!(std::fs::read(&saved).unwrap(), original);
        let mut expired = portable::tests::fixture(true);
        expired.access_token_expiry = 1.0;
        let expired_path = dir.path().join("expired.qwc");
        portable::export(&expired_path, expired).unwrap();
        assert!(manager.import_credentials(expired_path).await.is_err());
        assert_eq!(std::fs::read(&saved).unwrap(), original);
        drop(manager);

        let restarted =
            SessionManager::new(dir.path().join("no-quick")).with_credential_file(saved.clone());
        assert_eq!(restarted.credential_source().await, "imported");
        assert_eq!(restarted.catalog_identity().await.unwrap(), identity);
        let current = restarted.session(&client).await.unwrap();
        restarted.reject(&current).await;
        assert!(
            restarted
                .session(&client)
                .await
                .err()
                .unwrap()
                .to_string()
                .contains("服务端拒绝")
        );
        restarted.forget_credentials().await.unwrap();
        assert!(!saved.exists());
        assert!(exported.exists());
        assert_eq!(restarted.credential_source().await, "local");

        std::fs::write(&saved, b"{broken").unwrap();
        let damaged =
            SessionManager::new(dir.path().join("no-quick")).with_credential_file(saved.clone());
        assert_eq!(damaged.credential_source().await, "imported");
        assert!(damaged.catalog_identity().await.is_err());
        assert!(
            damaged
                .session(&client)
                .await
                .err()
                .unwrap()
                .to_string()
                .contains("重新导入")
        );
        damaged.forget_credentials().await.unwrap();
    }

    #[tokio::test]
    async fn imported_social_refresh_persists_rotation_and_reuses_it_after_restart() {
        let count = Arc::new(AtomicUsize::new(0));
        let calls = count.clone();
        let app = Router::new().route("/token", post(move |Json(body): Json<serde_json::Value>| {
            calls.fetch_add(1, Ordering::SeqCst);
            assert_eq!(body["refreshToken"], "synthetic-refresh");
            async { Json(serde_json::json!({"accessToken":"rotated-access", "expiresIn":3600, "refreshToken":"rotated-refresh"})) }
        }));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let endpoint = format!("http://{}/token", listener.local_addr().unwrap());
        let task = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("store/saved.qwc");
        let mut fixture = portable::tests::fixture(false);
        fixture.access_token_expiry = 1.0;
        let mut imported = PortableSession::from_session(fixture).unwrap();
        let manager =
            SessionManager::new(dir.path().join("no-quick")).with_credential_file(path.clone());
        let client = Client::new();
        // A missing destination directory simulates a save failure after successful rotation.
        assert!(
            manager
                .imported_session(&mut imported, &client, &endpoint)
                .await
                .is_err()
        );
        assert!(imported.dirty);
        assert_eq!(imported.session.refresh_token, "rotated-refresh");
        std::fs::create_dir(dir.path().join("store")).unwrap();
        manager
            .imported_session(&mut imported, &client, &endpoint)
            .await
            .unwrap();
        assert!(!imported.dirty);
        assert_eq!(count.load(Ordering::SeqCst), 1);
        let persisted = PortableSession::decode(&portable::read(&path).unwrap()).unwrap();
        assert_eq!(persisted.session.refresh_token, "rotated-refresh");
        let restarted = SessionManager::new(dir.path().join("no-quick")).with_credential_file(path);
        assert_eq!(
            restarted.session(&client).await.unwrap().access_token,
            "rotated-access"
        );
        task.abort();
    }

    fn fixture() -> QuickSession {
        QuickSession {
            managed_by_quick: false,
            access_token: "old-access-fixture".into(),
            access_token_expiry: 1.0,
            refresh_token: "old-refresh-fixture".into(),
            refresh_token_expiry: auth::now() + 3600.0,
            client_id: "client-fixture".into(),
            client_secret: "secret-fixture".into(),
            client_secret_expires_at: auth::now() + 3600.0,
            issuer_url: "https://example.invalid".into(),
            tenant_url: "https://tenant.example.invalid".into(),
            aws_account_id: "fixture".into(),
        }
    }

    fn valid_enterprise(token: &str) -> QuickSession {
        QuickSession {
            managed_by_quick: true,
            access_token: token.into(),
            access_token_expiry: auth::now() + 3600.0,
            ..fixture()
        }
    }

    #[cfg(target_os = "macos")]
    #[tokio::test]
    #[ignore = "native Keychain import and reuse using a unique synthetic profile"]
    async fn native_authorized_session_survives_source_keychain_item_removal_until_expiry() {
        use security_framework::passwords::{delete_generic_password, set_generic_password};
        let id = format!("enterprise-quickway-test-{}", uuid::Uuid::new_v4());
        let service = format!("quickwork-enterprise-{id}");
        let base = std::env::temp_dir().join(&id);
        std::fs::create_dir(&base).unwrap();
        struct Cleanup(PathBuf, String);
        impl Drop for Cleanup {
            fn drop(&mut self) {
                let _ = delete_generic_password(&self.1, "session");
                let _ = std::fs::remove_dir_all(&self.0);
            }
        }
        let _cleanup = Cleanup(base.clone(), service.clone());
        std::fs::write(
            base.join("profiles.json"),
            serde_json::json!({
                "last_active":id,"entries":[{"id":id,"provider":"enterprise","created_at":1}]
            })
            .to_string(),
        )
        .unwrap();
        let raw = serde_json::json!({
            "access_token":"synthetic-authorized-once", "access_token_expiry":auth::now()+600.0,
            "refresh_token_expiry":auth::now()+3600.0, "tenant_url":"https://tenant.example.invalid",
            "issuer":"https://issuer.example.invalid", "aws_account_id":"fixture"
        });
        set_generic_password(&service, "session", raw.to_string().as_bytes()).unwrap();
        let manager = SessionManager::new(base);
        let client = Client::new();
        manager.authorize(&client).await.unwrap();
        delete_generic_password(&service, "session").unwrap();
        for _ in 0..20 {
            assert_eq!(
                manager.session(&client).await.unwrap().access_token,
                "synthetic-authorized-once"
            );
        }
        manager
            .cached
            .lock()
            .await
            .as_mut()
            .unwrap()
            .session
            .access_token_expiry = 1.0;
        assert!(manager.session(&client).await.is_err());
        assert!(manager.cached.lock().await.is_none());
    }

    #[tokio::test]
    async fn mac_session_reuses_the_refreshed_social_token_and_rotates_only_at_expiry() {
        let calls = Arc::new(AtomicUsize::new(0));
        let count = calls.clone();
        let app = Router::new().route("/token", post(move |Json(body): Json<serde_json::Value>| {
            let attempt = count.fetch_add(1, Ordering::SeqCst);
            async move {
                assert_eq!(body["refreshToken"], if attempt == 0 { "old-refresh-fixture" } else { "rotated-fixture" });
                Json(serde_json::json!({"accessToken":"fresh-access-fixture","refreshToken":"rotated-fixture","expiresIn":3600}))
            }
        }));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let endpoint = format!("http://{}/token", listener.local_addr().unwrap());
        let server = tokio::spawn(async { axum::serve(listener, app).await.unwrap() });
        let manager = SessionManager::new(PathBuf::new());
        let client = Client::builder().no_proxy().build().unwrap();
        let disk = fixture();
        manager
            .profile_session_with(
                &client,
                false,
                || Ok([1; 32]),
                || async { Ok(disk.clone()) },
                &endpoint,
            )
            .await
            .unwrap();
        assert!(
            manager
                .profile_session_with(
                    &client,
                    false,
                    || Ok([1; 32]),
                    || async { Err(anyhow!("Keychain denied")) },
                    &endpoint
                )
                .await
                .is_ok()
        );
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        manager
            .cached
            .lock()
            .await
            .as_mut()
            .unwrap()
            .session
            .access_token_expiry = 1.0;
        // The on-disk token is unchanged; continue with the rotated refresh token.
        let renewed = manager
            .profile_session_with(
                &client,
                false,
                || Ok([1; 32]),
                || async { Ok(disk) },
                &endpoint,
            )
            .await
            .unwrap();
        assert_eq!(renewed.refresh_token, "rotated-fixture");
        assert_eq!(calls.load(Ordering::SeqCst), 2);
        server.abort();
    }

    #[tokio::test]
    async fn authorization_is_reused_for_sequential_and_concurrent_requests_without_keychain_reads()
    {
        let manager = SessionManager::new(PathBuf::new());
        let client = Client::new();
        let reads = AtomicUsize::new(0);
        let load = || async {
            assert_eq!(
                reads.fetch_add(1, Ordering::SeqCst),
                0,
                "Keychain read repeated"
            );
            Ok(valid_enterprise("authorized-once"))
        };
        manager
            .profile_session_with(&client, true, || Ok([1; 32]), load, TOKEN_ENDPOINT)
            .await
            .unwrap();
        for _ in 0..20 {
            let session = manager
                .profile_session_with(&client, false, || Ok([1; 32]), load, TOKEN_ENDPOINT)
                .await
                .unwrap();
            assert_eq!(session.access_token, "authorized-once");
        }
        let (chat, status, catalog) = tokio::join!(
            manager.profile_session_with(&client, false, || Ok([1; 32]), load, TOKEN_ENDPOINT),
            manager.profile_session_with(&client, false, || Ok([1; 32]), load, TOKEN_ENDPOINT),
            manager.profile_session_with(&client, false, || Ok([1; 32]), load, TOKEN_ENDPOINT),
        );
        assert!(chat.is_ok() && status.is_ok() && catalog.is_ok());
        assert_eq!(reads.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn concurrent_first_requests_only_import_once() {
        let manager = SessionManager::new(PathBuf::new());
        let client = Client::new();
        let reads = AtomicUsize::new(0);
        let load = || async {
            reads.fetch_add(1, Ordering::SeqCst);
            tokio::task::yield_now().await;
            Ok(valid_enterprise("cold-start"))
        };
        let (first, second) = tokio::join!(
            manager.profile_session_with(&client, false, || Ok([1; 32]), load, TOKEN_ENDPOINT),
            manager.profile_session_with(&client, false, || Ok([1; 32]), load, TOKEN_ENDPOINT),
        );
        assert!(first.is_ok() && second.is_ok());
        assert_eq!(reads.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn expired_tokens_require_a_fresh_read_and_never_fall_back_to_expired_credentials() {
        let manager = SessionManager::new(PathBuf::new());
        let client = Client::new();
        manager
            .profile_session_with(
                &client,
                true,
                || Ok([1; 32]),
                || async { Ok(valid_enterprise("first")) },
                TOKEN_ENDPOINT,
            )
            .await
            .unwrap();
        manager
            .cached
            .lock()
            .await
            .as_mut()
            .unwrap()
            .session
            .access_token_expiry = 1.0;
        let renewed = manager
            .profile_session_with(
                &client,
                false,
                || Ok([1; 32]),
                || async { Ok(valid_enterprise("quick-renewed")) },
                TOKEN_ENDPOINT,
            )
            .await
            .unwrap();
        assert_eq!(renewed.access_token, "quick-renewed");
        manager
            .cached
            .lock()
            .await
            .as_mut()
            .unwrap()
            .session
            .access_token_expiry = 1.0;
        assert!(
            manager
                .profile_session_with(
                    &client,
                    false,
                    || Ok([1; 32]),
                    || async { Err(anyhow!("Keychain denied")) },
                    TOKEN_ENDPOINT
                )
                .await
                .is_err()
        );
        assert!(manager.cached.lock().await.is_none());
        let mut expired = valid_enterprise("expired");
        expired.access_token_expiry = 1.0;
        assert!(
            manager
                .profile_session_with(
                    &client,
                    false,
                    || Ok([1; 32]),
                    || async { Ok(expired) },
                    "http://127.0.0.1:1"
                )
                .await
                .is_err()
        );
        assert!(manager.cached.lock().await.is_none());
    }

    #[tokio::test]
    async fn profile_switch_removal_and_switch_during_authorization_clear_old_credentials() {
        let manager = SessionManager::new(PathBuf::new());
        let client = Client::new();
        let load = || async { Ok(valid_enterprise("account-one")) };
        manager
            .profile_session_with(&client, true, || Ok([1; 32]), load, TOKEN_ENDPOINT)
            .await
            .unwrap();
        assert!(
            manager
                .profile_session_with(
                    &client,
                    false,
                    || Ok([2; 32]),
                    || async { Err(anyhow!("account two unauthorized")) },
                    TOKEN_ENDPOINT
                )
                .await
                .is_err()
        );
        assert!(manager.cached.lock().await.is_none());
        manager
            .profile_session_with(&client, true, || Ok([1; 32]), load, TOKEN_ENDPOINT)
            .await
            .unwrap();
        assert!(
            manager
                .profile_session_with(
                    &client,
                    false,
                    || Err(anyhow!("profile removed")),
                    load,
                    TOKEN_ENDPOINT
                )
                .await
                .is_err()
        );
        assert!(manager.cached.lock().await.is_none());
        let profile = AtomicUsize::new(1);
        let result = manager
            .profile_session_with(
                &client,
                true,
                || Ok([profile.load(Ordering::SeqCst) as u8; 32]),
                || async {
                    profile.store(2, Ordering::SeqCst);
                    Ok(valid_enterprise("account-one"))
                },
                TOKEN_ENDPOINT,
            )
            .await;
        assert!(result.is_err());
        assert!(manager.cached.lock().await.is_none());
    }

    #[tokio::test]
    async fn a_late_unauthorized_response_does_not_erase_a_newer_authorization() {
        let manager = SessionManager::new(PathBuf::new());
        let client = Client::new();
        let old = manager
            .profile_session_with(
                &client,
                true,
                || Ok([1; 32]),
                || async { Ok(valid_enterprise("old")) },
                TOKEN_ENDPOINT,
            )
            .await
            .unwrap();
        let new = manager
            .profile_session_with(
                &client,
                true,
                || Ok([1; 32]),
                || async { Ok(valid_enterprise("new")) },
                TOKEN_ENDPOINT,
            )
            .await
            .unwrap();
        manager.reject(&old).await;
        let cached = manager
            .profile_session_with(
                &client,
                false,
                || Ok([1; 32]),
                || async { Err(anyhow!("Keychain denied")) },
                TOKEN_ENDPOINT,
            )
            .await
            .unwrap();
        assert_eq!(cached.access_token, "new");
        manager.reject(&new).await;
        assert!(
            manager
                .profile_session_with(
                    &client,
                    false,
                    || Ok([1; 32]),
                    || async { Err(anyhow!("Keychain denied")) },
                    TOKEN_ENDPOINT
                )
                .await
                .is_err()
        );
        assert!(manager.cached.lock().await.is_none());
    }

    #[tokio::test]
    async fn cancelling_a_manual_reauthorization_preserves_the_still_valid_session() {
        let manager = SessionManager::new(PathBuf::new());
        let client = Client::new();
        manager
            .profile_session_with(
                &client,
                true,
                || Ok([1; 32]),
                || async { Ok(valid_enterprise("current")) },
                TOKEN_ENDPOINT,
            )
            .await
            .unwrap();
        assert!(
            manager
                .profile_session_with(
                    &client,
                    true,
                    || Ok([1; 32]),
                    || async { Err(anyhow!("cancelled")) },
                    TOKEN_ENDPOINT
                )
                .await
                .is_err()
        );
        assert!(
            manager
                .profile_session_with(
                    &client,
                    false,
                    || Ok([1; 32]),
                    || async { Err(anyhow!("Keychain denied")) },
                    TOKEN_ENDPOINT
                )
                .await
                .is_ok()
        );
    }

    #[tokio::test]
    async fn refresh_is_single_flight_and_quick_relogin_invalidates_cache() {
        let calls = Arc::new(AtomicUsize::new(0));
        let count = calls.clone();
        let app = Router::new().route("/token", post(move |Json(body): Json<serde_json::Value>| {
            let count = count.clone();
            async move {
                let attempt = count.fetch_add(1, Ordering::SeqCst);
                assert_eq!(body["grantType"], "refresh_token");
                assert_eq!(body["clientId"], "client-fixture");
                assert_eq!(body["clientSecret"], "secret-fixture");
                assert_eq!(body["refreshToken"], if attempt == 0 { "old-refresh-fixture" } else { "rotated-fixture" });
                Json(serde_json::json!({"accessToken":"fresh-access-fixture", "refreshToken":"rotated-fixture", "expiresIn":3600}))
            }
        }));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let endpoint = format!("http://{}/token", listener.local_addr().unwrap());
        let server = tokio::spawn(async { axum::serve(listener, app).await.unwrap() });
        let manager = SessionManager::new(PathBuf::new());
        let client = Client::builder().no_proxy().build().unwrap();
        let disk = fixture();
        let (first, second) = tokio::join!(
            manager.session_with(&client, || Ok(disk.clone()), &endpoint),
            manager.session_with(&client, || Ok(disk.clone()), &endpoint),
        );
        assert_eq!(first.unwrap().access_token, "fresh-access-fixture");
        assert_eq!(second.unwrap().refresh_token, "rotated-fixture");
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        manager
            .cached
            .lock()
            .await
            .as_mut()
            .unwrap()
            .session
            .access_token_expiry = 1.0;
        manager
            .session_with(&client, || Ok(disk.clone()), &endpoint)
            .await
            .unwrap();
        assert_eq!(calls.load(Ordering::SeqCst), 2);
        let mut new_login = disk.clone();
        new_login.access_token = "quick-new-login-fixture".into();
        new_login.access_token_expiry = auth::now() + 3600.0;
        let session = manager
            .session_with(&client, || Ok(new_login), &endpoint)
            .await
            .unwrap();
        assert_eq!(session.access_token, "quick-new-login-fixture");
        assert_eq!(calls.load(Ordering::SeqCst), 2);
        assert!(
            manager
                .session_with(&client, || Err(anyhow!("signed out")), &endpoint)
                .await
                .is_err()
        );
        assert!(manager.cached.lock().await.is_none());
        server.abort();
    }

    #[tokio::test]
    async fn enterprise_expiry_never_uses_social_refresh_and_quick_renewal_is_observed() {
        let manager = SessionManager::new(PathBuf::new());
        let client = Client::builder().no_proxy().build().unwrap();
        let mut disk = fixture();
        disk.managed_by_quick = true;
        let error = manager
            .session_with(&client, || Ok(disk.clone()), "http://127.0.0.1:1")
            .await
            .err()
            .unwrap();
        assert!(error.to_string().contains("打开 Amazon Quick"));
        disk.access_token = "renewed-by-quick".into();
        disk.access_token_expiry = auth::now() + 600.0;
        assert_eq!(
            manager
                .session_with(&client, || Ok(disk), "http://127.0.0.1:1")
                .await
                .unwrap()
                .access_token,
            "renewed-by-quick"
        );
    }

    #[tokio::test]
    async fn missing_expiry_and_error_bodies_are_not_accepted_or_exposed() {
        let app = Router::new()
            .route(
                "/missing",
                post(|| async { Json(serde_json::json!({"accessToken":"DO_NOT_EXPOSE"})) }),
            )
            .route(
                "/failure",
                post(|| async { (axum::http::StatusCode::BAD_REQUEST, "DO_NOT_EXPOSE") }),
            );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let server = tokio::spawn(async { axum::serve(listener, app).await.unwrap() });
        let client = Client::builder().no_proxy().build().unwrap();
        for path in ["missing", "failure"] {
            let error = refresh(&fixture(), &client, &format!("{base}/{path}"))
                .await
                .err()
                .unwrap();
            assert!(!format!("{error:#}").contains("DO_NOT_EXPOSE"));
        }
        server.abort();
    }
}
