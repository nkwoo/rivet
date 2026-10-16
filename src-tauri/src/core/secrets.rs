//! OS 보안 저장소(keyring) 래퍼. 1단계에서 구현합니다.
//!
//! 키 형식: `rivet:<profile_id>`. 비밀 값은 `secrecy`/`zeroize`로 다루고 `Debug`에서 마스킹합니다.
