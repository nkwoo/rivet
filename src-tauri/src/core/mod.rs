//! 프로토콜과 무관한 앱 핵심 로직.

pub mod app_info;
pub mod cert_pins;
pub mod db;
pub mod known_hosts;
pub mod profile;
pub mod secrets;
pub mod session;
pub mod ssh_config;

use std::path::Path;
use std::sync::Arc;

use crate::error::Result;
use cert_pins::CertPinStore;
use db::Database;
use known_hosts::KnownHostsStore;
use profile::ProfileStore;
use secrets::{KeyringStore, SecretStore};
use session::SessionManager;

use crate::transfer::TransferManager;

/// Tauri가 관리하는 앱 전역 상태
pub struct AppState {
    pub profiles: ProfileStore,
    pub known_hosts: Arc<KnownHostsStore>,
    pub cert_pins: Arc<CertPinStore>,
    pub secrets: Arc<dyn SecretStore>,
    pub sessions: Arc<SessionManager>,
    pub transfers: Arc<TransferManager>,
}

impl AppState {
    /// 앱 데이터 디렉터리의 `rivet.db`를 열고 OS 키체인을 비밀 값 저장소로 사용합니다.
    pub fn open(data_dir: &Path) -> Result<Self> {
        std::fs::create_dir_all(data_dir)?;
        let db = Arc::new(Database::open(&data_dir.join("rivet.db"))?);
        Ok(Self::with(db, Arc::new(KeyringStore)))
    }

    pub fn with(db: Arc<Database>, secrets: Arc<dyn SecretStore>) -> Self {
        Self {
            profiles: ProfileStore::new(db.clone()),
            known_hosts: Arc::new(KnownHostsStore::new(db.clone())),
            cert_pins: Arc::new(CertPinStore::new(db)),
            secrets,
            sessions: SessionManager::new(),
            transfers: TransferManager::new(),
        }
    }

    /// 프로필과 함께 keyring에 저장된 비밀 값도 지웁니다.
    pub fn delete_profile(&self, id: &str) -> Result<()> {
        self.profiles.delete(id)?;
        if let Err(e) = self.secrets.delete(id) {
            tracing::warn!(error = %e, "profile deleted but keychain entry could not be removed");
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::profile::{AuthMethod, ProfileInput, Protocol};
    use crate::core::secrets::MemoryStore;
    use secrecy::SecretString;

    #[test]
    fn deleting_profile_removes_its_secret() {
        let secrets = Arc::new(MemoryStore::default());
        let state = AppState::with(
            Arc::new(Database::open_in_memory().unwrap()),
            secrets.clone(),
        );
        let p = state
            .profiles
            .create(ProfileInput {
                name: "web".into(),
                protocol: Protocol::Ssh,
                host: "h".into(),
                port: 22,
                username: "u".into(),
                auth_method: AuthMethod::Password,
                key_path: None,
                group: None,
                tags: vec![],
                notes: String::new(),
                favorite: false,
                rdp: None,
            })
            .unwrap();
        secrets.set(&p.id, &SecretString::from("pw")).unwrap();
        state.delete_profile(&p.id).unwrap();
        assert!(secrets.get(&p.id).unwrap().is_none());
    }
}
