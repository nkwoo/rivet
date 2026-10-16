//! 텍스트 클립보드 공유 (CLIPRDR).
//!
//! IronRDP의 클립보드 채널은 백엔드 콜백을 동기적으로 부르므로, 콜백은 이벤트만 큐에 넣고
//! 실제 처리(OS 클립보드 읽기/쓰기, 응답 PDU 전송)는 세션 루프가 합니다.
//! 텍스트(CF_UNICODETEXT)만 다루고 파일·이미지는 주고받지 않습니다.

use ironrdp::cliprdr::backend::CliprdrBackend;
use ironrdp::cliprdr::pdu::{
    ClipboardFormat, ClipboardFormatId, ClipboardGeneralCapabilityFlags, FileContentsRequest,
    FileContentsResponse, FormatDataRequest, FormatDataResponse, LockDataId,
};
use tokio::sync::mpsc;

/// 주고받는 텍스트 크기 상한 (UTF-16 바이트)
pub const MAX_CLIPBOARD_BYTES: usize = 4 * 1024 * 1024;

/// 백엔드 콜백 → 세션 루프
#[derive(Debug)]
pub enum ClipEvent {
    /// 채널 준비 완료 또는 서버가 클라이언트 형식 목록을 요청함: 로컬 텍스트가 있으면 알림
    Announce,
    /// 원격에서 복사함
    RemoteCopy(Vec<ClipboardFormatId>),
    /// 원격에서 붙여넣기 하려고 로컬 데이터를 요청함
    DataRequest(ClipboardFormatId),
    /// 앞서 요청한 원격 데이터 (`None`이면 실패)
    DataResponse(Option<Vec<u8>>),
}

/// 콜백을 이벤트로 바꾸는 백엔드
#[derive(Debug)]
pub struct ChannelBackend {
    events: mpsc::UnboundedSender<ClipEvent>,
}

impl ChannelBackend {
    pub fn new(events: mpsc::UnboundedSender<ClipEvent>) -> Self {
        Self { events }
    }

    fn send(&self, event: ClipEvent) {
        let _ = self.events.send(event);
    }
}

ironrdp::core::impl_as_any!(ChannelBackend);

impl CliprdrBackend for ChannelBackend {
    fn temporary_directory(&self) -> &str {
        // 파일 복사를 지원하지 않으므로 쓰이지 않습니다.
        ""
    }

    fn client_capabilities(&self) -> ClipboardGeneralCapabilityFlags {
        ClipboardGeneralCapabilityFlags::USE_LONG_FORMAT_NAMES
    }

    fn on_ready(&mut self) {
        self.send(ClipEvent::Announce);
    }

    fn on_request_format_list(&mut self) {
        self.send(ClipEvent::Announce);
    }

    fn on_process_negotiated_capabilities(&mut self, _: ClipboardGeneralCapabilityFlags) {}

    fn on_remote_copy(&mut self, available_formats: &[ClipboardFormat]) {
        self.send(ClipEvent::RemoteCopy(
            available_formats.iter().map(ClipboardFormat::id).collect(),
        ));
    }

    fn on_format_data_request(&mut self, request: FormatDataRequest) {
        self.send(ClipEvent::DataRequest(request.format));
    }

    fn on_format_data_response(&mut self, response: FormatDataResponse<'_>) {
        let data = (!response.is_error() && response.data().len() <= MAX_CLIPBOARD_BYTES)
            .then(|| response.data().to_vec());
        self.send(ClipEvent::DataResponse(data));
    }

    fn on_file_contents_request(&mut self, _: FileContentsRequest) {}

    fn on_file_contents_response(&mut self, _: FileContentsResponse<'_>) {}

    fn on_lock(&mut self, _: LockDataId) {}

    fn on_unlock(&mut self, _: LockDataId) {}
}

/// 원격이 보낸 CF_UNICODETEXT(UTF-16LE, NUL 종료)를 문자열로 바꿉니다.
pub fn decode_unicode_text(bytes: &[u8]) -> Option<String> {
    if bytes.len() > MAX_CLIPBOARD_BYTES {
        return None;
    }
    let units: Vec<u16> = bytes
        .as_chunks::<2>()
        .0
        .iter()
        .map(|c| u16::from_le_bytes(*c))
        .take_while(|&u| u != 0)
        .collect();
    Some(String::from_utf16_lossy(&units).replace("\r\n", "\n"))
}

/// 로컬 텍스트를 원격으로 보낼 수 있는지 (크기 상한)
pub fn fits(text: &str) -> bool {
    text.encode_utf16().count() * 2 + 2 <= MAX_CLIPBOARD_BYTES
}

/// 줄바꿈을 Windows 형식으로 맞춥니다.
pub fn to_windows_newlines(text: &str) -> String {
    text.replace("\r\n", "\n").replace('\n', "\r\n")
}

/// OS 클립보드 (테스트에서는 메모리 구현으로 바꿉니다)
pub trait LocalClipboard: Send + Sync {
    fn read_text(&self) -> Option<String>;
    fn write_text(&self, text: &str) -> bool;
}

/// arboard로 접근하는 시스템 클립보드. 호출은 블로킹이므로 `spawn_blocking`에서 씁니다.
pub struct SystemClipboard;

impl LocalClipboard for SystemClipboard {
    fn read_text(&self) -> Option<String> {
        arboard::Clipboard::new().ok()?.get_text().ok()
    }

    fn write_text(&self, text: &str) -> bool {
        match arboard::Clipboard::new() {
            Ok(mut c) => c.set_text(text).is_ok(),
            Err(_) => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_utf16_until_nul_and_normalizes_newlines() {
        let mut bytes: Vec<u8> = "한글\r\nok"
            .encode_utf16()
            .flat_map(u16::to_le_bytes)
            .collect();
        bytes.extend_from_slice(&[0, 0, b'x', 0]);
        assert_eq!(decode_unicode_text(&bytes).as_deref(), Some("한글\nok"));
        assert_eq!(
            decode_unicode_text(&vec![0x41; MAX_CLIPBOARD_BYTES + 2]),
            None
        );
    }

    #[test]
    fn size_limit_and_newlines() {
        assert!(fits("hello"));
        assert!(!fits(&"a".repeat(MAX_CLIPBOARD_BYTES / 2)));
        assert_eq!(to_windows_newlines("a\nb\r\nc"), "a\r\nb\r\nc");
    }

    #[test]
    fn backend_turns_callbacks_into_events() {
        let (tx, mut rx) = mpsc::unbounded_channel();
        let mut b = ChannelBackend::new(tx);
        b.on_ready();
        b.on_remote_copy(&[ClipboardFormat::new(ClipboardFormatId::CF_UNICODETEXT)]);
        b.on_format_data_response(FormatDataResponse::new_error());
        assert!(matches!(rx.try_recv(), Ok(ClipEvent::Announce)));
        assert!(
            matches!(rx.try_recv(), Ok(ClipEvent::RemoteCopy(f)) if f == [ClipboardFormatId::CF_UNICODETEXT])
        );
        assert!(matches!(rx.try_recv(), Ok(ClipEvent::DataResponse(None))));
    }
}
