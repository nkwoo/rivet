/**
 * 백엔드 화면 메시지(`protocols/rdp/frame.rs`)를 캔버스에 그립니다.
 * 모든 정수는 리틀 엔디언이고, 픽셀은 불투명 RGBA입니다.
 */

const KIND_FRAME = 1;
const KIND_POINTER = 2;
const KIND_POINTER_DEFAULT = 3;
const KIND_POINTER_HIDDEN = 4;

export type Pointer =
  | { kind: "bitmap"; width: number; height: number; hotX: number; hotY: number; rgba: Uint8ClampedArray<ArrayBuffer> }
  | { kind: "default" }
  | { kind: "hidden" };

export type Message =
  | { kind: "frame"; width: number; height: number; rects: number }
  | { kind: "pointer"; pointer: Pointer }
  | { kind: "unknown" };

/**
 * 메시지를 해석하고, 화면 메시지는 바로 그립니다. 원격 화면 크기가 바뀌면 캔버스 크기도 바꿉니다.
 * 형식이 맞지 않으면 그 메시지만 버립니다.
 */
export function applyMessage(bytes: Uint8Array<ArrayBuffer>, canvas: HTMLCanvasElement): Message {
  if (bytes.length < 2) return { kind: "unknown" };
  const view = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
  const u16 = (offset: number) => view.getUint16(offset, true);

  switch (bytes[0]) {
    case KIND_FRAME: {
      if (bytes.length < 8) return { kind: "unknown" };
      const width = u16(2);
      const height = u16(4);
      const count = u16(6);
      if (canvas.width !== width || canvas.height !== height) {
        canvas.width = width;
        canvas.height = height;
      }
      const ctx = canvas.getContext("2d", { alpha: false });
      if (!ctx) return { kind: "unknown" };
      let offset = 8;
      for (let i = 0; i < count; i++) {
        if (offset + 8 > bytes.length) break;
        const x = u16(offset);
        const y = u16(offset + 2);
        const w = u16(offset + 4);
        const h = u16(offset + 6);
        offset += 8;
        const len = w * h * 4;
        if (w === 0 || h === 0 || offset + len > bytes.length) break;
        const pixels = new Uint8ClampedArray(bytes.buffer, bytes.byteOffset + offset, len);
        ctx.putImageData(new ImageData(pixels, w, h), x, y);
        offset += len;
      }
      return { kind: "frame", width, height, rects: count };
    }
    case KIND_POINTER: {
      if (bytes.length < 10) return { kind: "unknown" };
      const width = u16(2);
      const height = u16(4);
      const len = width * height * 4;
      if (width === 0 || height === 0 || bytes.length < 10 + len) return { kind: "unknown" };
      return {
        kind: "pointer",
        pointer: {
          kind: "bitmap",
          width,
          height,
          hotX: u16(6),
          hotY: u16(8),
          rgba: new Uint8ClampedArray(bytes.slice(10, 10 + len)),
        },
      };
    }
    case KIND_POINTER_DEFAULT:
      return { kind: "pointer", pointer: { kind: "default" } };
    case KIND_POINTER_HIDDEN:
      return { kind: "pointer", pointer: { kind: "hidden" } };
    default:
      return { kind: "unknown" };
  }
}

/** 서버 포인터 이미지를 CSS 커서로 바꿉니다. `scale`은 원격 픽셀 ÷ CSS 픽셀 (Retina 접속이면 2). */
export function cursorCss(pointer: Pointer, scale: number): string {
  if (pointer.kind === "default") return "default";
  if (pointer.kind === "hidden") return "none";
  const w = Math.max(1, Math.round(pointer.width / scale));
  const h = Math.max(1, Math.round(pointer.height / scale));
  // 브라우저 커서 크기 상한 (128px)
  if (w > 128 || h > 128) return "default";
  const source = document.createElement("canvas");
  source.width = pointer.width;
  source.height = pointer.height;
  source.getContext("2d")?.putImageData(new ImageData(pointer.rgba, pointer.width, pointer.height), 0, 0);
  let target = source;
  if (scale !== 1) {
    target = document.createElement("canvas");
    target.width = w;
    target.height = h;
    const ctx = target.getContext("2d");
    if (ctx) {
      ctx.imageSmoothingQuality = "high";
      ctx.drawImage(source, 0, 0, w, h);
    }
  }
  const hx = Math.min(w - 1, Math.round(pointer.hotX / scale));
  const hy = Math.min(h - 1, Math.round(pointer.hotY / scale));
  return `url(${target.toDataURL("image/png")}) ${hx} ${hy}, default`;
}
