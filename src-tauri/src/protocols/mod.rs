//! 네트워크 프로토콜 구현. 각 모듈은 공통 trait(`RemoteSession`, `RemoteFs`)을 구현합니다.

pub mod ftp;
pub mod rdp;
pub mod sftp;
pub mod ssh;

use async_trait::async_trait;

/// 프로토콜과 무관한 원격 연결의 수명주기
#[async_trait]
pub trait RemoteSession: Send + Sync {
    /// 연결을 정상 종료합니다. 이미 끊긴 연결이면 아무 일도 하지 않습니다.
    async fn disconnect(&self);
}
