//! 네트워크 프로토콜 구현. 각 모듈은 공통 trait(`RemoteSession`, `RemoteFs`)을 구현합니다.

pub mod ftp;
pub mod rdp;
pub mod sftp;
pub mod ssh;
