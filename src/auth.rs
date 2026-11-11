//! Read only the active Amazon Quick credential. Never log secret values.
use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

#[derive(Deserialize)]
struct Manifest {
    last_active: String,
    entries: Vec<Profile>,
}
#[derive(Deserialize)]
struct Profile {
    id: String,
    provider: String,
    #[serde(default)]
    created_at: serde_json::Value,
    #[serde(default)]
    login_metadata: serde_json::Value,
}

// Deliberately does not implement Debug.
#[derive(Clone, Deserialize, Serialize)]
pub struct QuickSession {
    #[serde(skip)]
    pub managed_by_quick: bool,
    /// The bearer presented upstream: the ID token for enterprise logins that
    /// have one, otherwise the access token. `access_token_expiry` follows it.
    pub access_token: String,
    pub access_token_expiry: f64,
    pub refresh_token: String,
    pub refresh_token_expiry: f64,
    pub client_id: String,
    pub client_secret: String,
    pub client_secret_expires_at: f64,
    pub issuer_url: String,
    pub tenant_url: String,
    pub aws_account_id: String,
}

#[derive(Deserialize)]
struct EnterpriseSession {
    access_token: String,
    access_token_expiry: f64,
    #[serde(default)]
    id_token: Option<String>,
    #[serde(default)]
    id_token_expiry: Option<f64>,
    refresh_token_expiry: f64,
    tenant_url: String,
    issuer: String,
    aws_account_id: String,
}

fn parse_session(raw: &str, enterprise: bool) -> Result<QuickSession> {
    if !enterprise {
        return serde_json::from_str(raw)
            .map_err(|_| anyhow::anyhow!("Quick session has an unsupported schema"));
    }
    let session: EnterpriseSession = serde_json::from_str(raw)
        .map_err(|_| anyhow::anyhow!("Quick enterprise session has an unsupported schema"))?;
    // Match Quick's enterprise provider: present the ID token whenever the
    // credential has one and judge the session by its expiry; the DataPlane
    // rejects the OAuth access token there. Only credentials without an ID
    // token fall back to the access token. An expired ID token must not
    // silently switch to the still-valid access token.
    let (bearer, expiry) = match session.id_token.filter(|token| !token.is_empty()) {
        Some(id_token) => (id_token, session.id_token_expiry.unwrap_or(0.0)),
        None => (session.access_token, session.access_token_expiry),
    };
    Ok(QuickSession {
        managed_by_quick: true,
        access_token: bearer,
        access_token_expiry: expiry,
        refresh_token: String::new(),
        refresh_token_expiry: session.refresh_token_expiry,
        client_id: String::new(),
        client_secret: String::new(),
        client_secret_expires_at: 0.0,
        issuer_url: session.issuer,
        tenant_url: session.tenant_url,
        aws_account_id: session.aws_account_id,
    })
}

pub fn now() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs_f64()
}

pub fn quick_base() -> Result<PathBuf> {
    #[cfg(windows)]
    let home = std::env::var_os("USERPROFILE").or_else(|| std::env::var_os("HOME"));
    #[cfg(not(windows))]
    let home = std::env::var_os("HOME");
    let home = home.context("Cannot determine user home; pass --quick-base")?;
    Ok(PathBuf::from(home).join(".quickwork"))
}

pub fn active_profile(base: &Path) -> Result<String> {
    Ok(read_profile(base)?.id)
}

/// Scope non-secret model metadata to the active Quick profile. Token renewal
/// and Keychain access failures do not mean that the user switched accounts.
pub(crate) fn profile_identity(base: &Path) -> Result<[u8; 32]> {
    let profile = read_profile(base)?;
    let identity = serde_json::to_vec(&(
        profile.id,
        profile.provider,
        profile.created_at,
        profile.login_metadata,
    ))?;
    Ok(Sha256::digest(identity).into())
}

fn read_profile(base: &Path) -> Result<Profile> {
    let bytes = std::fs::read(base.join("profiles.json"))
        .context("Quick profiles.json not found; sign in to Amazon Quick first")?;
    parse_profile(&bytes)
}

fn parse_profile(bytes: &[u8]) -> Result<Profile> {
    let manifest: Manifest =
        serde_json::from_slice(bytes).context("Invalid Quick profile manifest")?;
    let profile = manifest
        .entries
        .into_iter()
        .find(|p| p.id == manifest.last_active)
        .context("Quick active profile not found")?;
    let prefix = match profile.provider.as_str() {
        "social" | "social_gateway" => "social-",
        "enterprise" => "enterprise-",
        _ => bail!("当前 Quick 登录通道尚不支持，请使用 App Integrations 社交或企业登录。"),
    };
    if !profile.id.starts_with(prefix)
        || !profile
            .id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-')
    {
        bail!("Invalid Quick profile identifier")
    }
    Ok(profile)
}

pub fn load_session(base: &Path) -> Result<QuickSession> {
    with_keychain_interaction(false, || read_session(base))
}

/// Only call in response to an explicit local authorization action. Background
/// status checks and model requests must never open a macOS password dialog.
pub fn authorize_session(base: &Path) -> Result<QuickSession> {
    with_keychain_interaction(true, || read_session(base))
}

fn with_keychain_interaction<T>(interactive: bool, read: impl FnOnce() -> Result<T>) -> Result<T> {
    #[cfg(target_os = "macos")]
    {
        use security_framework::os::macos::keychain::SecKeychain;
        // The Security framework flag is process-wide. Serialize our reads so
        // an explicit authorization cannot race with a background lookup.
        static ACCESS: std::sync::Mutex<()> = std::sync::Mutex::new(());
        let _access = ACCESS.lock().unwrap_or_else(|e| e.into_inner());
        let _quiet = if !interactive && SecKeychain::user_interaction_allowed()? {
            Some(SecKeychain::disable_user_interaction()?)
        } else {
            None
        };
        read()
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = interactive;
        read()
    }
}

fn credential_target(profile: &Profile) -> (bool, String) {
    let enterprise = profile.provider == "enterprise";
    let kind = if enterprise { "enterprise" } else { "social" };
    (
        enterprise,
        format!("session@quickwork-{kind}-{}", profile.id),
    )
}

fn read_session(base: &Path) -> Result<QuickSession> {
    let profile = read_profile(base)?;
    let (enterprise, target) = credential_target(&profile);
    let raw = assemble_credential(&target, read_credential)?;
    parse_session(&raw, enterprise)
}

/// Non-secret facts about Quick's profile manifest, for diagnostics.
pub(crate) fn profile_summary(base: &Path) -> serde_json::Value {
    let path = base.join("profiles.json");
    let bytes = match std::fs::read(&path) {
        Ok(bytes) => bytes,
        Err(error) => {
            return serde_json::json!({"quick_base_exists":base.is_dir(),"error":format!("profiles.json: {}", error.kind())});
        }
    };
    let raw: serde_json::Value = serde_json::from_slice(&bytes).unwrap_or_default();
    let entries = raw["entries"]
        .as_array()
        .map(Vec::as_slice)
        .unwrap_or_default();
    let active = raw["last_active"].as_str().unwrap_or_default();
    let providers: Vec<_> = entries.iter().map(|e| e["provider"].clone()).collect();
    let active_entry = entries.iter().find(|e| e["id"].as_str() == Some(active));
    let mut summary = serde_json::json!({
        "quick_base_exists": true,
        "entries": entries.len(),
        "providers": providers,
        "active_found": active_entry.is_some(),
    });
    if let Some(entry) = active_entry {
        let keys = |value: &serde_json::Value| -> Vec<String> {
            value
                .as_object()
                .map(|o| o.keys().cloned().collect())
                .unwrap_or_default()
        };
        summary["active_provider"] = entry["provider"].clone();
        summary["active_entry_keys"] = keys(entry).into();
        summary["login_metadata_keys"] = keys(&entry["login_metadata"]).into();
        // Quick records when its own background refresh started failing.
        if let Some(since) = entry.get("refresh_failing_since").and_then(|v| v.as_f64()) {
            summary["quick_refresh_failing_for_s"] = (now() - since).round().into();
        } else if let Some(since) = entry.get("refresh_failing_since") {
            summary["quick_refresh_failing_since"] = since.clone();
        }
        // Region and similar routing hints are not personal data.
        for key in ["region", "idp_type", "identity_provider", "login_type"] {
            if let Some(value) = entry["login_metadata"].get(key).filter(|v| v.is_string()) {
                summary[format!("login_metadata_{key}")] = value.clone();
            }
        }
    }
    if let Err(error) = parse_profile(&bytes) {
        summary["error"] = error.to_string().into();
    }
    summary
}

pub(crate) struct LocalCredential {
    pub enterprise: bool,
    pub records: usize,
    pub raw: zeroize::Zeroizing<String>,
}

/// Read the active Quick credential without any interactive prompt, for diagnostics only.
pub(crate) fn read_local_credential(base: &Path) -> Result<LocalCredential> {
    with_keychain_interaction(false, || {
        let profile = read_profile(base)?;
        let (enterprise, target) = credential_target(&profile);
        let records = std::cell::Cell::new(0);
        let raw = assemble_credential(&target, |name| {
            records.set(records.get() + 1);
            read_credential(name)
        })?;
        Ok(LocalCredential {
            enterprise,
            records: records.get(),
            raw: raw.into(),
        })
    })
}

pub(crate) fn parse_local_session(raw: &str, enterprise: bool) -> Result<QuickSession> {
    parse_session(raw, enterprise)
}

fn assemble_credential(target: &str, read: impl Fn(&str) -> Result<String>) -> Result<String> {
    let first = read(target)?;
    let value: serde_json::Value =
        serde_json::from_str(&first).context("Invalid Quick credential record")?;
    let Some(count) = value.get("__chunks__") else {
        return Ok(first);
    };
    let count = count
        .as_u64()
        .filter(|v| (1..=64).contains(v))
        .context("Invalid Quick credential chunk count")?;
    let expected = value["__sha__"]
        .as_str()
        .context("Missing Quick credential checksum")?;
    let mut combined = String::new();
    for i in 0..count {
        combined.push_str(&read(&format!("{target}_chunk_{i}"))?);
    }
    let actual = format!("{:x}", Sha256::digest(combined.as_bytes()));
    if actual != expected {
        bail!("Quick credential changed while reading; retry after sign-in completes")
    }
    Ok(combined)
}

/// Resolve `account@service` the way Python keyring's Windows backend does:
/// the plain `service` record wins when its user name is `account`; the
/// compound `account@service` record is only a fallback. keyring moves the
/// previous plain record there on each write, so it can hold an expired session.
#[cfg(any(windows, test))]
fn keyring_lookup(
    target: &str,
    read: impl Fn(&str) -> Result<Option<(String, String)>>,
) -> Result<Option<String>> {
    if let Some((account, service)) = target.split_once('@')
        && let Some((user, secret)) = read(service)?
        && user == account
    {
        return Ok(Some(secret));
    }
    Ok(read(target)?.map(|(_, secret)| secret))
}

#[cfg(windows)]
fn read_credential(target: &str) -> Result<String> {
    keyring_lookup(target, read_windows_credential)?
        .context("Quick credential is unavailable in Windows Credential Manager; sign in to Quick")
}

/// Read one generic credential, returning its user name and decoded secret.
/// `Ok(None)` means the record does not exist.
#[cfg(windows)]
fn read_windows_credential(target: &str) -> Result<Option<(String, String)>> {
    use windows_sys::Win32::Security::Credentials::{
        CRED_TYPE_GENERIC, CREDENTIALW, CredFree, CredReadW,
    };
    let name: Vec<u16> = target.encode_utf16().chain(Some(0)).collect();
    let mut ptr: *mut CREDENTIALW = std::ptr::null_mut();
    // CredRead returns an OS-owned allocation, freed on all paths before parsing.
    let (user, bytes) = unsafe {
        if CredReadW(name.as_ptr(), CRED_TYPE_GENERIC, 0, &mut ptr) == 0 {
            return Ok(None);
        }
        let size = (*ptr).CredentialBlobSize as usize;
        if size > 65536 || !size.is_multiple_of(2) || (size != 0 && (*ptr).CredentialBlob.is_null())
        {
            CredFree(ptr.cast());
            bail!("Invalid Quick credential blob")
        }
        let user_ptr = (*ptr).UserName;
        let user = if user_ptr.is_null() {
            String::new()
        } else {
            let len = (0..).take_while(|&i| *user_ptr.add(i) != 0).count();
            String::from_utf16_lossy(std::slice::from_raw_parts(user_ptr, len))
        };
        let result = if size == 0 {
            Vec::new()
        } else {
            std::slice::from_raw_parts((*ptr).CredentialBlob, size).to_vec()
        };
        CredFree(ptr.cast());
        (user, result)
    };
    let units: Vec<u16> = bytes
        .as_chunks::<2>()
        .0
        .iter()
        .map(|b| u16::from_le_bytes([b[0], b[1]]))
        .collect();
    let secret = String::from_utf16(&units).context("Invalid Quick credential encoding")?;
    Ok(Some((user, secret)))
}

#[cfg(target_os = "macos")]
fn read_credential(target: &str) -> Result<String> {
    // Python keyring uses account@service on Windows, but separate fields on macOS.
    // Keep the service suffix on chunk records, matching Quick's credential layout.
    let (account, service) = target
        .split_once('@')
        .context("Invalid credential target")?;
    let bytes =
        security_framework::passwords::get_generic_password(service, account).map_err(|error| {
            match error.code() {
                -25300 => {
                    anyhow::anyhow!("Mac 钥匙串中没有 Quick 登录凭据，请先登录 Amazon Quick。")
                }
                -25293 | -25308 | -128 => anyhow::anyhow!(
                    "Quick 钥匙串尚未授权或已锁定。请点击「授权钥匙串」，在系统窗口输入 Mac 登录密码并选择「始终允许」。"
                ),
                _ => anyhow::anyhow!("读取 Quick 钥匙串失败（系统错误 {}）。", error.code()),
            }
        })?;
    String::from_utf8(bytes).context("Invalid Quick credential encoding")
}

#[cfg(not(any(windows, target_os = "macos")))]
fn read_credential(_: &str) -> Result<String> {
    bail!("Quick session import requires Windows or macOS")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(target_os = "macos")]
    #[test]
    fn background_keychain_reads_suppress_dialogs_and_restore_the_previous_policy() {
        use security_framework::os::macos::keychain::SecKeychain;
        let before = SecKeychain::user_interaction_allowed().unwrap();
        let result: Result<()> = with_keychain_interaction(false, || {
            assert!(!SecKeychain::user_interaction_allowed().unwrap());
            bail!("synthetic read failure")
        });
        assert!(result.is_err());
        assert_eq!(SecKeychain::user_interaction_allowed().unwrap(), before);
    }

    #[test]
    fn profile_selection_is_scoped_to_supported_provider_and_safe_identifier() {
        for (provider, id, valid) in [
            ("social", "social-test", true),
            ("social_gateway", "social-test", true),
            ("enterprise", "enterprise-test", true),
            ("gateway", "gateway-test", false),
            ("enterprise", "social-test", false),
            ("social", "social-../../other", false),
            ("social", "social-test@other", false),
        ] {
            let manifest =
                serde_json::json!({"last_active":id,"entries":[{"id":id,"provider":provider}]});
            assert_eq!(
                parse_profile(&serde_json::to_vec(&manifest).unwrap()).is_ok(),
                valid
            );
        }
        assert!(parse_profile(br#"{"last_active":"missing","entries":[]}"#).is_err());
    }

    #[test]
    fn catalog_profile_identity_does_not_require_credentials_or_change_on_activity() {
        let base = std::env::temp_dir().join(format!("quickway-profile-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&base).unwrap();
        let path = base.join("profiles.json");
        let mut manifest = serde_json::json!({"last_active":"enterprise-fixture","entries":[{
            "id":"enterprise-fixture","provider":"enterprise","created_at":"fixture-time",
            "login_metadata":{"user":"fixture-user"},"last_used_at":1
        }]});
        let save = |manifest: &serde_json::Value| {
            std::fs::write(&path, serde_json::to_vec(manifest).unwrap()).unwrap()
        };
        save(&manifest);
        let first = profile_identity(&base).unwrap();
        manifest["entries"][0]["last_used_at"] = serde_json::json!(2);
        manifest["entries"][0]["display_name"] = serde_json::json!("Renamed profile");
        save(&manifest);
        assert_eq!(profile_identity(&base).unwrap(), first);
        manifest["entries"][0]["login_metadata"]["user"] = serde_json::json!("other-user");
        save(&manifest);
        assert_ne!(profile_identity(&base).unwrap(), first);
        manifest["entries"] = serde_json::json!([]);
        save(&manifest);
        assert!(profile_identity(&base).is_err());
        std::fs::remove_dir_all(base).unwrap();
    }

    #[test]
    fn enterprise_session_uses_access_token_and_leaves_refresh_to_quick() {
        let raw = r#"{"access_token":"fixture-access","access_token_expiry":100,"refresh_token_expiry":200,"tenant_url":"https://tenant.example.invalid","issuer":"https://issuer.example.invalid","aws_account_id":"fixture","refresh_token":"not-imported"}"#;
        let session = parse_session(raw, true).unwrap();
        assert!(session.managed_by_quick);
        assert_eq!(session.access_token, "fixture-access");
        assert!(session.refresh_token.is_empty());
        assert!(parse_session(raw, false).is_err());
        let malformed = raw.replace(":100", ":\"DO_NOT_EXPOSE\"");
        assert!(
            !format!("{:#}", parse_session(&malformed, true).err().unwrap())
                .contains("DO_NOT_EXPOSE")
        );
    }

    #[test]
    fn enterprise_session_presents_id_token_and_follows_its_expiry() {
        let raw = r#"{"access_token":"fixture-access","access_token_expiry":900,"id_token":"fixture-id","id_token_expiry":300,"refresh_token_expiry":200,"tenant_url":"https://tenant.example.invalid","issuer":"https://issuer.example.invalid","aws_account_id":"fixture"}"#;
        let session = parse_session(raw, true).unwrap();
        assert_eq!(session.access_token, "fixture-id");
        assert_eq!(session.access_token_expiry, 300.0);
        // An ID token without a recorded expiry is treated as expired rather
        // than borrowing the access token's lifetime.
        let no_expiry = raw.replace(r#","id_token_expiry":300"#, "");
        let session = parse_session(&no_expiry, true).unwrap();
        assert_eq!(session.access_token, "fixture-id");
        assert_eq!(session.access_token_expiry, 0.0);
        // Empty or null ID tokens fall back to the access token.
        for empty in [r#""id_token":"""#, r#""id_token":null"#] {
            let session =
                parse_session(&raw.replace(r#""id_token":"fixture-id""#, empty), true).unwrap();
            assert_eq!(session.access_token, "fixture-access");
            assert_eq!(session.access_token_expiry, 900.0);
        }
    }

    #[cfg(target_os = "macos")]
    #[test]
    #[ignore = "opt-in native Keychain roundtrip using a unique synthetic credential"]
    fn macos_keychain_roundtrip_and_chunk_layout() {
        use security_framework::passwords::{delete_generic_password, set_generic_password};
        let service = format!("quickway-test-{}", uuid::Uuid::new_v4());
        struct Cleanup(String);
        impl Drop for Cleanup {
            fn drop(&mut self) {
                for suffix in ["", "_chunk_0", "_chunk_1"] {
                    let _ = delete_generic_password(&format!("{}{suffix}", self.0), "session");
                }
            }
        }
        let _cleanup = Cleanup(service.clone());
        let data = r#"{"access_token":"synthetic-中文"}"#;
        let marker = serde_json::json!({"__chunks__":2,"__sha__":format!("{:x}",Sha256::digest(data.as_bytes()))}).to_string();
        set_generic_password(&service, "session", marker.as_bytes()).unwrap();
        set_generic_password(
            &format!("{service}_chunk_0"),
            "session",
            &data.as_bytes()[..10],
        )
        .unwrap();
        set_generic_password(
            &format!("{service}_chunk_1"),
            "session",
            &data.as_bytes()[10..],
        )
        .unwrap();
        assert_eq!(
            assemble_credential(&format!("session@{service}"), read_credential).unwrap(),
            data
        );
    }
    #[test]
    fn chunk_integrity_and_rotation() {
        let session = r#"{"access_token":"test-only-你好"}"#;
        let marker = serde_json::json!({"__chunks__":2,"__sha__":format!("{:x}",Sha256::digest(session.as_bytes()))}).to_string();
        let assembled = assemble_credential("fixture", |key| {
            Ok(match key {
                "fixture" => marker.clone(),
                "fixture_chunk_0" => session[..10].to_string(),
                _ => session[10..].to_string(),
            })
        })
        .unwrap();
        assert_eq!(assembled, session);
        assert!(
            assemble_credential("fixture", |key| Ok(if key == "fixture" {
                marker.clone()
            } else {
                "changed".into()
            }))
            .is_err()
        );
    }
    #[test]
    fn bounds_chunk_count() {
        assert!(
            assemble_credential("fixture", |_| Ok(
                r#"{"__chunks__":1000000,"__sha__":"x"}"#.into()
            ))
            .is_err()
        );
    }
    #[test]
    fn keyring_lookup_prefers_plain_service_record_like_python_keyring() {
        let store = |records: &'static [(&'static str, &'static str, &'static str)]| {
            move |target: &str| -> Result<Option<(String, String)>> {
                Ok(records
                    .iter()
                    .find(|(name, _, _)| *name == target)
                    .map(|(_, user, secret)| (user.to_string(), secret.to_string())))
            }
        };
        // Quick rewrites the plain record; a stale compound record must not win.
        let both = store(&[
            ("svc", "session", "fresh"),
            ("session@svc", "session", "stale"),
        ]);
        assert_eq!(
            keyring_lookup("session@svc", both).unwrap().as_deref(),
            Some("fresh")
        );
        // A plain record that belongs to another account is ignored.
        let other = store(&[
            ("svc", "someone", "other"),
            ("session@svc", "session", "mine"),
        ]);
        assert_eq!(
            keyring_lookup("session@svc", other).unwrap().as_deref(),
            Some("mine")
        );
        // Older layouts with only the compound record keep working.
        let compound = store(&[("session@svc", "session", "only")]);
        assert_eq!(
            keyring_lookup("session@svc", compound).unwrap().as_deref(),
            Some("only")
        );
        assert!(keyring_lookup("session@svc", store(&[])).unwrap().is_none());
        assert!(keyring_lookup("session@svc", |_| bail!("unreadable")).is_err());
    }
}
