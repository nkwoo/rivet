//! 화면 갱신을 웹뷰로 보내는 바이너리 메시지.
//!
//! 바뀐 영역(dirty rect)만 RGBA로 잘라 보냅니다. 모든 정수는 리틀 엔디언입니다.
//!
//! ```text
//! 화면  : u8 KIND_FRAME, u8 0, u16 desktop_w, u16 desktop_h, u16 rect_count,
//!         rect_count × (u16 x, u16 y, u16 w, u16 h, w*h*4 바이트 RGBA)
//! 포인터: u8 KIND_POINTER, u8 0, u16 w, u16 h, u16 hotspot_x, u16 hotspot_y, w*h*4 바이트 RGBA
//! 기본 포인터: u8 KIND_POINTER_DEFAULT, u8 0
//! 포인터 숨김: u8 KIND_POINTER_HIDDEN, u8 0
//! ```
//!
//! 헤더가 8바이트 단위라 픽셀 데이터는 항상 4바이트 경계에서 시작합니다.

use ironrdp::pdu::geometry::InclusiveRectangle;

pub const KIND_FRAME: u8 = 1;
pub const KIND_POINTER: u8 = 2;
pub const KIND_POINTER_DEFAULT: u8 = 3;
pub const KIND_POINTER_HIDDEN: u8 = 4;

/// 사각형이 이보다 많으면 하나로 합쳐 보냅니다.
const MAX_RECTS: usize = 64;

/// 화면 위의 영역 (끝 좌표 미포함)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rect {
    pub x: u16,
    pub y: u16,
    pub w: u16,
    pub h: u16,
}

impl Rect {
    /// IronRDP의 포함 사각형을 화면 크기로 잘라 변환합니다. 화면 밖이면 `None`.
    pub fn clipped(r: &InclusiveRectangle, width: u16, height: u16) -> Option<Self> {
        let right = r.right.min(width.checked_sub(1)?);
        let bottom = r.bottom.min(height.checked_sub(1)?);
        if r.left > right || r.top > bottom {
            return None;
        }
        Some(Self {
            x: r.left,
            y: r.top,
            w: right - r.left + 1,
            h: bottom - r.top + 1,
        })
    }

    fn area(self) -> u64 {
        u64::from(self.w) * u64::from(self.h)
    }

    fn union(self, other: Self) -> Self {
        let x = self.x.min(other.x);
        let y = self.y.min(other.y);
        let right =
            (u32::from(self.x) + u32::from(self.w)).max(u32::from(other.x) + u32::from(other.w));
        let bottom =
            (u32::from(self.y) + u32::from(self.h)).max(u32::from(other.y) + u32::from(other.h));
        Self {
            x,
            y,
            w: u16::try_from(right - u32::from(x)).unwrap_or(u16::MAX),
            h: u16::try_from(bottom - u32::from(y)).unwrap_or(u16::MAX),
        }
    }
}

/// 다음 전송까지 쌓인 변경 영역
#[derive(Debug, Default)]
pub struct DirtyRegion {
    rects: Vec<Rect>,
}

impl DirtyRegion {
    pub fn add(&mut self, rect: Rect) {
        if self.rects.iter().any(|r| contains(*r, rect)) {
            return;
        }
        self.rects.retain(|r| !contains(rect, *r));
        self.rects.push(rect);
        if self.rects.len() > MAX_RECTS {
            self.collapse();
        }
    }

    pub fn is_empty(&self) -> bool {
        self.rects.is_empty()
    }

    pub fn clear(&mut self) {
        self.rects.clear();
    }

    /// 보낼 사각형을 꺼냅니다. 겹침이 많아 합친 사각형이 더 작으면 하나로 보냅니다.
    pub fn take(&mut self) -> Vec<Rect> {
        let total: u64 = self.rects.iter().map(|r| r.area()).sum();
        if let Some(bounds) = self.bounds() {
            if self.rects.len() > 1 && bounds.area() <= total {
                self.rects.clear();
                return vec![bounds];
            }
        }
        std::mem::take(&mut self.rects)
    }

    fn bounds(&self) -> Option<Rect> {
        self.rects.iter().copied().reduce(Rect::union)
    }

    fn collapse(&mut self) {
        if let Some(bounds) = self.bounds() {
            self.rects = vec![bounds];
        }
    }
}

fn contains(outer: Rect, inner: Rect) -> bool {
    outer.x <= inner.x
        && outer.y <= inner.y
        && u32::from(outer.x) + u32::from(outer.w) >= u32::from(inner.x) + u32::from(inner.w)
        && u32::from(outer.y) + u32::from(outer.h) >= u32::from(inner.y) + u32::from(inner.h)
}

/// RGBA 프레임 버퍼의 지정한 영역을 화면 메시지로 만듭니다.
///
/// `rects`는 [`Rect::clipped`]로 화면 안에 있어야 합니다.
pub fn encode_frame(data: &[u8], width: u16, height: u16, rects: &[Rect]) -> Vec<u8> {
    let stride = usize::from(width) * 4;
    let pixels: usize = rects
        .iter()
        .map(|r| usize::from(r.w) * usize::from(r.h) * 4 + 8)
        .sum();
    let mut out = Vec::with_capacity(8 + pixels);
    out.extend_from_slice(&[KIND_FRAME, 0]);
    out.extend_from_slice(&width.to_le_bytes());
    out.extend_from_slice(&height.to_le_bytes());
    let count = u16::try_from(rects.len()).unwrap_or(u16::MAX);
    out.extend_from_slice(&count.to_le_bytes());
    for r in rects.iter().take(usize::from(count)) {
        for v in [r.x, r.y, r.w, r.h] {
            out.extend_from_slice(&v.to_le_bytes());
        }
        let row = usize::from(r.w) * 4;
        for y in usize::from(r.y)..usize::from(r.y) + usize::from(r.h) {
            let start = y * stride + usize::from(r.x) * 4;
            match data.get(start..start + row) {
                // IronRDP 디코더는 그리는 픽셀의 알파를 항상 0xFF로 씁니다. 한 번도 그려지지 않은 영역만
                // 초기값 0(검은색)이라 그대로 보내도 화면은 같으므로 줄 단위로 복사만 합니다.
                Some(line) => out.extend_from_slice(line),
                // 버퍼보다 큰 사각형은 들어오지 않지만, 들어와도 패닉 대신 검은색으로 채웁니다.
                None => out.extend((0..row).map(|i| if i % 4 == 3 { 0xFF } else { 0 })),
            }
        }
    }
    out
}

/// 서버가 보낸 마우스 포인터 이미지 (RGBA)
pub fn encode_pointer(
    width: u16,
    height: u16,
    hotspot_x: u16,
    hotspot_y: u16,
    rgba: &[u8],
) -> Option<Vec<u8>> {
    let len = usize::from(width) * usize::from(height) * 4;
    let pixels = rgba.get(..len)?;
    let mut out = Vec::with_capacity(10 + len);
    out.extend_from_slice(&[KIND_POINTER, 0]);
    for v in [width, height, hotspot_x, hotspot_y] {
        out.extend_from_slice(&v.to_le_bytes());
    }
    out.extend_from_slice(pixels);
    Some(out)
}

pub fn encode_pointer_default() -> Vec<u8> {
    vec![KIND_POINTER_DEFAULT, 0]
}

pub fn encode_pointer_hidden() -> Vec<u8> {
    vec![KIND_POINTER_HIDDEN, 0]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect(x: u16, y: u16, w: u16, h: u16) -> Rect {
        Rect { x, y, w, h }
    }

    #[test]
    fn clips_to_desktop_and_rejects_outside() {
        let r = InclusiveRectangle {
            left: 10,
            top: 20,
            right: 2000,
            bottom: 30,
        };
        assert_eq!(Rect::clipped(&r, 100, 100), Some(rect(10, 20, 90, 11)));
        let outside = InclusiveRectangle {
            left: 200,
            top: 0,
            right: 300,
            bottom: 10,
        };
        assert_eq!(Rect::clipped(&outside, 100, 100), None);
        assert_eq!(Rect::clipped(&r, 0, 0), None);
    }

    #[test]
    fn region_drops_contained_rects_and_merges_overlaps() {
        let mut d = DirtyRegion::default();
        d.add(rect(0, 0, 10, 10));
        d.add(rect(2, 2, 3, 3));
        assert_eq!(d.take(), vec![rect(0, 0, 10, 10)]);
        assert!(d.is_empty());

        // 서로 떨어진 작은 사각형은 따로 보냄
        d.add(rect(0, 0, 2, 2));
        d.add(rect(50, 50, 2, 2));
        assert_eq!(d.take().len(), 2);

        // 크게 겹치면 합친 사각형 하나
        d.add(rect(0, 0, 10, 10));
        d.add(rect(5, 0, 10, 10));
        assert_eq!(d.take(), vec![rect(0, 0, 15, 10)]);
    }

    #[test]
    fn region_collapses_when_too_many_rects() {
        let mut d = DirtyRegion::default();
        for i in 0..=MAX_RECTS as u16 {
            d.add(rect(i * 3, 0, 1, 1));
        }
        assert_eq!(d.take(), vec![rect(0, 0, 193, 1)]);
    }

    #[test]
    fn encodes_rect_pixels() {
        // 4x2 화면, 픽셀 값 = 인덱스
        let data: Vec<u8> = (0..4 * 2)
            .flat_map(|i| [i as u8, i as u8, i as u8, 255])
            .collect();
        let msg = encode_frame(&data, 4, 2, &[rect(1, 0, 2, 2)]);
        assert_eq!(&msg[..8], &[KIND_FRAME, 0, 4, 0, 2, 0, 1, 0]);
        assert_eq!(&msg[8..16], &[1, 0, 0, 0, 2, 0, 2, 0]);
        assert_eq!(
            &msg[16..],
            &[1, 1, 1, 255, 2, 2, 2, 255, 5, 5, 5, 255, 6, 6, 6, 255]
        );
    }

    #[test]
    fn pointer_requires_enough_pixels() {
        assert!(encode_pointer(2, 2, 0, 0, &[0; 15]).is_none());
        let msg = encode_pointer(2, 2, 1, 1, &[7; 16]).unwrap();
        assert_eq!(&msg[..10], &[KIND_POINTER, 0, 2, 0, 2, 0, 1, 0, 1, 0]);
        assert_eq!(msg.len(), 26);
    }
}
