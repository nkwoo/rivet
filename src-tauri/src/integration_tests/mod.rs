//! 실제 서버(Docker)가 필요한 통합 테스트. 기본 `cargo test`에서는 `#[ignore]`로 건너뜁니다.
//! RDP는 프로세스 안에서 IronRDP 서버를 띄우므로 기본 실행에 포함됩니다.

mod ftp_integration;
mod rdp_integration;
mod ssh_integration;
