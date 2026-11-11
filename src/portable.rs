//! Explicitly exported QuickWay credentials. Files contain secrets; never log their contents.
use crate::{auth::QuickSession, upstream};
use anyhow::{Result, anyhow, bail, ensure};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    io::{Read, Write},
    path::Path,
};
use zeroize::Zeroizing;

const FORMAT: &str = "quickway.credentials";
const VERSION: u32 = 1;
const MAX_FILE: usize = 1024 * 1024;

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Contents {
    format: String,
    version: u32,
    identity: [u8; 32],
    managed_by_quick: bool,
    session: QuickSession,
}

// Secret-bearing types intentionally have no Debug implementation.
pub(crate) struct PortableSession {
    pub identity: [u8; 32],
    pub session: QuickSession,
    pub rejected: bool,
    pub dirty: bool,
}

fn validate(session: &QuickSession) -> Result<()> {
    ensure!(!session.access_token.is_empty(), "凭证缺少访问令牌。");
    ensure!(
        [
            session.access_token_expiry,
            session.refresh_token_expiry,
            session.client_secret_expires_at,
        ]
        .iter()
        .all(|value| value.is_finite() && *value >= 0.0),
        "凭证有效期无效。"
    );
    upstream::tenant_url(session, "/")
        .map_err(|_| anyhow!("凭证不是受支持的 Quick 生产环境登录。"))?;
    Ok(())
}

impl PortableSession {
    pub fn from_session(session: QuickSession) -> Result<Self> {
        validate(&session)?;
        Ok(Self {
            identity: Sha256::digest(uuid::Uuid::new_v4().as_bytes()).into(),
            session,
            rejected: false,
            dirty: false,
        })
    }

    pub fn decode(bytes: &[u8]) -> Result<Self> {
        ensure!(bytes.len() <= MAX_FILE, "凭证文件过大。");
        let mut contents: Contents = serde_json::from_slice(bytes)
            .map_err(|_| anyhow!("文件不是受支持的 QuickWay 凭证，或文件已损坏。"))?;
        ensure!(
            contents.format == FORMAT && contents.version == VERSION,
            "不支持此凭证文件版本，请使用新版 QuickWay 重新导出。"
        );
        contents.session.managed_by_quick = contents.managed_by_quick;
        validate(&contents.session)?;
        Ok(Self {
            identity: contents.identity,
            session: contents.session,
            rejected: false,
            dirty: false,
        })
    }

    pub fn encode(&self) -> Result<Zeroizing<Vec<u8>>> {
        let contents = Contents {
            format: FORMAT.into(),
            version: VERSION,
            identity: self.identity,
            managed_by_quick: self.session.managed_by_quick,
            session: self.session.clone(),
        };
        let bytes = Zeroizing::new(
            serde_json::to_vec_pretty(&contents).map_err(|_| anyhow!("无法编码凭证。"))?,
        );
        ensure!(bytes.len() <= MAX_FILE, "凭证文件过大。");
        Ok(bytes)
    }
}

pub(crate) fn read(path: &Path) -> Result<Zeroizing<Vec<u8>>> {
    let mut bytes = Zeroizing::new(Vec::new());
    std::fs::File::open(path)
        .map_err(|_| anyhow!("无法打开凭证文件。"))?
        .take(MAX_FILE as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| anyhow!("无法读取凭证文件。"))?;
    ensure!(bytes.len() <= MAX_FILE, "凭证文件过大。");
    Ok(bytes)
}

/// Explicit export without overwriting any existing file. No password is required.
pub fn export(path: &Path, session: QuickSession) -> Result<()> {
    let bytes = PortableSession::from_session(session)?.encode()?;
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options
        .open(path)
        .map_err(|_| anyhow!("无法创建导出文件；请选择一个尚不存在的文件名。"))?;
    if file
        .write_all(&bytes)
        .and_then(|_| file.sync_all())
        .is_err()
    {
        drop(file);
        let _ = std::fs::remove_file(path);
        bail!("无法保存导出文件，请检查空间和权限。");
    }
    Ok(())
}

/// Atomically replace the app-owned copy after token rotation. Unix mode is 0600;
/// Windows inherits the application's per-user configuration directory permissions.
pub(crate) fn save(path: &Path, imported: &PortableSession) -> Result<()> {
    let bytes = imported.encode()?;
    let parent = path.parent().ok_or_else(|| anyhow!("凭证保存位置无效。"))?;
    let mut file =
        tempfile::NamedTempFile::new_in(parent).map_err(|_| anyhow!("无法创建凭证文件。"))?;
    file.write_all(&bytes)
        .and_then(|_| file.as_file().sync_all())
        .map_err(|_| anyhow!("无法保存凭证。"))?;
    file.persist(path)
        .map_err(|_| anyhow!("无法替换凭证文件。"))?;
    Ok(())
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    pub(crate) fn fixture(enterprise: bool) -> QuickSession {
        QuickSession {
            managed_by_quick: enterprise,
            access_token: "synthetic-secret-access".into(),
            access_token_expiry: crate::auth::now() + 3600.0,
            refresh_token: "synthetic-refresh".into(),
            refresh_token_expiry: crate::auth::now() + 7200.0,
            client_id: "synthetic-client".into(),
            client_secret: "synthetic-secret-client".into(),
            client_secret_expires_at: crate::auth::now() + 7200.0,
            issuer_url: "https://issuer.example.invalid".into(),
            tenant_url: "https://fixture.dp.appintegrations.us-east-1.prod.plato.ai.aws.dev".into(),
            aws_account_id: "fixture-account".into(),
        }
    }

    #[test]
    fn portable_roundtrip_and_invalid_files() {
        let imported = PortableSession::from_session(fixture(true)).unwrap();
        let bytes = imported.encode().unwrap();
        assert!(String::from_utf8_lossy(&bytes).contains("synthetic-secret-access"));
        let decoded = PortableSession::decode(&bytes).unwrap();
        assert!(decoded.session.managed_by_quick);
        assert_eq!(decoded.identity, imported.identity);
        assert_eq!(decoded.session.access_token, imported.session.access_token);
        assert!(PortableSession::decode(&bytes[..bytes.len() - 1]).is_err());
        assert!(PortableSession::decode(&vec![0; MAX_FILE + 1]).is_err());
        let mut invalid: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        invalid["version"] = serde_json::json!(2);
        assert!(PortableSession::decode(&serde_json::to_vec(&invalid).unwrap()).is_err());
        let error = PortableSession::decode(b"{synthetic-secret-access")
            .err()
            .unwrap()
            .to_string();
        assert!(!error.contains("synthetic-secret-access"));
    }

    #[test]
    fn exports_validate_and_never_overwrite_existing_files() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("fixture.qwc");
        let mut malicious = fixture(false);
        malicious.tenant_url = "https://example.invalid".into();
        assert!(export(&path, malicious).is_err());
        export(&path, fixture(false)).unwrap();
        let original = read(&path).unwrap();
        assert!(export(&path, fixture(false)).is_err());
        assert_eq!(*read(&path).unwrap(), *original);
        let decoded = PortableSession::decode(&original).unwrap();
        assert!(!decoded.session.managed_by_quick);
        assert_eq!(decoded.session.refresh_token, "synthetic-refresh");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
                0o600
            );
            save(&path, &decoded).unwrap();
            assert_eq!(
                std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
    }
}
