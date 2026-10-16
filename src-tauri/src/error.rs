/// 백엔드 공통 에러 타입. 프로토콜/저장소 에러는 단계별로 변형을 추가합니다.
///
/// 비밀 값(비밀번호, 패스프레이즈)은 에러 메시지에 절대 포함하지 않습니다.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("tauri: {0}")]
    Tauri(#[from] tauri::Error),
}
