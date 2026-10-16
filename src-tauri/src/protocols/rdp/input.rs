//! 웹뷰에서 들어온 키보드/마우스 입력을 검증해 IronRDP 입력 연산으로 바꿉니다.
//!
//! 키보드는 물리 키 위치(스캔 코드)로 보내 원격 키보드 배열을 따릅니다. 한글처럼 로컬 IME가
//! 조합한 글자는 유니코드 입력으로 보냅니다.

use ironrdp::input::{MouseButton, MousePosition, Operation, Scancode, WheelRotations};
use serde::Deserialize;

/// 한 번에 받는 입력 이벤트 수 상한
pub const MAX_EVENTS: usize = 256;
/// 유니코드 입력 한 번의 글자 수 상한 (IME 조합 결과, 붙여넣기 아님)
const MAX_TEXT_CHARS: usize = 256;

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(tag = "t", rename_all = "camelCase", deny_unknown_fields)]
pub enum InputEvent {
    /// 마우스 이동 (원격 화면 좌표)
    #[serde(rename = "m")]
    Move { x: u16, y: u16 },
    /// 마우스 버튼. `MouseEvent.button` 값 (0 왼쪽, 1 가운데, 2 오른쪽, 3 뒤로, 4 앞으로)
    #[serde(rename = "b")]
    Button { button: u8, down: bool },
    /// 휠. `delta`는 RDP 회전 단위(한 칸 = 120), 아래/오른쪽이 음수
    #[serde(rename = "w")]
    Wheel { vertical: bool, delta: i16 },
    /// 스캔 코드. 확장 키는 0xE0xx
    #[serde(rename = "k")]
    Key { code: u16, down: bool },
    /// IME가 조합을 마친 글자들 (누름과 뗌을 함께 보냄)
    #[serde(rename = "u")]
    Text { text: String },
    /// 포커스를 잃음: 눌린 키와 버튼을 모두 뗌
    #[serde(rename = "r")]
    ReleaseAll,
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum InputError {
    #[error("too many input events")]
    TooMany,
    #[error("invalid mouse button {0}")]
    Button(u8),
    #[error("invalid scancode {0:#06x}")]
    Scancode(u16),
    #[error("invalid text input")]
    Text,
}

/// 입력 처리 결과
#[derive(Debug, Default)]
pub struct Translated {
    pub operations: Vec<Operation>,
    /// `ReleaseAll`이 들어 있었는지 (호출자가 `Database::release_all`을 실행)
    pub release_all: bool,
}

fn button(value: u8) -> Result<MouseButton, InputError> {
    Ok(match value {
        0 => MouseButton::Left,
        1 => MouseButton::Middle,
        2 => MouseButton::Right,
        3 => MouseButton::X1,
        4 => MouseButton::X2,
        other => return Err(InputError::Button(other)),
    })
}

fn scancode(code: u16) -> Result<Scancode, InputError> {
    let high = code >> 8;
    let low = code & 0xFF;
    // 하위 바이트는 0x01~0x7F (상위 비트는 키 뗌 표시라 쓰지 않음), 상위 바이트는 없거나 0xE0
    if !(0x01..=0x7F).contains(&low) || !(high == 0 || high == 0xE0) {
        return Err(InputError::Scancode(code));
    }
    Ok(Scancode::from_u16(code))
}

/// 웹뷰 입력을 검증하고 변환합니다. 좌표는 원격 화면 크기 안으로 맞춥니다.
pub fn translate(events: &[InputEvent], width: u16, height: u16) -> Result<Translated, InputError> {
    if events.len() > MAX_EVENTS {
        return Err(InputError::TooMany);
    }
    let mut out = Translated::default();
    for event in events {
        match event {
            InputEvent::Move { x, y } => out.operations.push(Operation::MouseMove(MousePosition {
                x: (*x).min(width.saturating_sub(1)),
                y: (*y).min(height.saturating_sub(1)),
            })),
            InputEvent::Button { button: b, down } => {
                let b = button(*b)?;
                out.operations.push(if *down {
                    Operation::MouseButtonPressed(b)
                } else {
                    Operation::MouseButtonReleased(b)
                });
            }
            InputEvent::Wheel { vertical, delta } => {
                if *delta != 0 {
                    out.operations
                        .push(Operation::WheelRotations(WheelRotations {
                            is_vertical: *vertical,
                            rotation_units: *delta,
                        }));
                }
            }
            InputEvent::Key { code, down } => {
                let sc = scancode(*code)?;
                out.operations.push(if *down {
                    Operation::KeyPressed(sc)
                } else {
                    Operation::KeyReleased(sc)
                });
            }
            InputEvent::Text { text } => {
                if text.chars().count() > MAX_TEXT_CHARS || text.chars().any(char::is_control) {
                    return Err(InputError::Text);
                }
                for c in text.chars() {
                    out.operations.push(Operation::UnicodeKeyPressed(c));
                    out.operations.push(Operation::UnicodeKeyReleased(c));
                }
            }
            InputEvent::ReleaseAll => out.release_all = true,
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ops(events: &[InputEvent]) -> Vec<Operation> {
        translate(events, 800, 600).unwrap().operations
    }

    #[test]
    fn parses_compact_json() {
        let json = r#"[{"t":"m","x":5,"y":6},{"t":"k","code":57427,"down":true},{"t":"u","text":"한"},{"t":"r"}]"#;
        let events: Vec<InputEvent> = serde_json::from_str(json).unwrap();
        assert_eq!(
            events[1],
            InputEvent::Key {
                code: 0xE053,
                down: true
            }
        );
        assert!(
            serde_json::from_str::<Vec<InputEvent>>(r#"[{"t":"m","x":1,"y":2,"z":3}]"#).is_err()
        );
        assert!(serde_json::from_str::<Vec<InputEvent>>(r#"[{"t":"m","x":-1,"y":2}]"#).is_err());
    }

    #[test]
    fn clamps_mouse_to_desktop() {
        let o = ops(&[InputEvent::Move { x: 5000, y: 10 }]);
        assert!(matches!(
            o[0],
            Operation::MouseMove(MousePosition { x: 799, y: 10 })
        ));
    }

    #[test]
    fn validates_scancodes_and_buttons() {
        assert!(translate(
            &[InputEvent::Key {
                code: 0x1E,
                down: true
            }],
            1,
            1
        )
        .is_ok());
        assert!(translate(
            &[InputEvent::Key {
                code: 0xE05B,
                down: true
            }],
            1,
            1
        )
        .is_ok());
        for bad in [0, 0x80, 0xE000, 0x1E1E, 0xFFFF] {
            assert_eq!(
                translate(
                    &[InputEvent::Key {
                        code: bad,
                        down: true
                    }],
                    1,
                    1
                )
                .err(),
                Some(InputError::Scancode(bad))
            );
        }
        assert_eq!(
            translate(
                &[InputEvent::Button {
                    button: 9,
                    down: true
                }],
                1,
                1
            )
            .err(),
            Some(InputError::Button(9))
        );
    }

    #[test]
    fn text_is_pressed_and_released_per_char() {
        let o = ops(&[InputEvent::Text {
            text: "가a".into()
        }]);
        assert_eq!(o.len(), 4);
        assert!(matches!(o[0], Operation::UnicodeKeyPressed('가')));
        assert!(matches!(o[3], Operation::UnicodeKeyReleased('a')));
        assert_eq!(
            translate(
                &[InputEvent::Text {
                    text: "a\nb".into()
                }],
                1,
                1
            )
            .err(),
            Some(InputError::Text)
        );
        assert_eq!(
            translate(
                &[InputEvent::Text {
                    text: "a".repeat(300)
                }],
                1,
                1
            )
            .err(),
            Some(InputError::Text)
        );
    }

    #[test]
    fn rejects_too_many_events_and_flags_release_all() {
        let many = vec![InputEvent::Move { x: 0, y: 0 }; MAX_EVENTS + 1];
        assert_eq!(translate(&many, 1, 1).err(), Some(InputError::TooMany));
        let t = translate(&[InputEvent::ReleaseAll], 1, 1).unwrap();
        assert!(t.release_all && t.operations.is_empty());
    }
}
