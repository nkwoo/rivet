//! 실제 서버(Docker)가 필요한 통합 테스트. 기본 `cargo test`에서는 `#[ignore]`로 건너뜁니다.

mod ssh_integration;
