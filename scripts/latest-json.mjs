#!/usr/bin/env node
// 자동 업데이트가 읽는 latest.json을 만듭니다 (릴리스 워크플로).
//
//   node scripts/latest-json.mjs <파일 폴더> <vX.Y.Z> <owner/repo> [변경 내용 파일]
//
// 폴더에는 업데이트 파일과 서명(.sig)이 있어야 합니다.
//   macOS: Rivet_<버전>_universal.app.tar.gz(.sig)   Windows: *_x64-setup.exe(.sig), *_x64_en-US.msi(.sig)
// 앱은 `{os}-{arch}-{설치 방식}`을 먼저 찾고 없으면 `{os}-{arch}`를 씁니다.
import { existsSync, readFileSync, readdirSync, writeFileSync } from "node:fs";
import { join } from "node:path";

const [dir, tag, repo, notesFile] = process.argv.slice(2);
if (!dir || !tag || !repo) {
  console.error("사용법: node scripts/latest-json.mjs <폴더> <vX.Y.Z> <owner/repo> [변경 내용 파일]");
  process.exit(2);
}
const version = tag.replace(/^v/, "");
const files = readdirSync(dir);

function artifact(suffix) {
  const matches = files.filter((f) => f.endsWith(suffix));
  if (matches.length !== 1) throw new Error(`${suffix} 파일이 하나여야 합니다: ${matches.join(", ") || "없음"}`);
  const [name] = matches;
  const sig = join(dir, `${name}.sig`);
  if (!existsSync(sig)) throw new Error(`서명 파일이 없습니다: ${name}.sig`);
  return {
    signature: readFileSync(sig, "utf8").trim(),
    url: `https://github.com/${repo}/releases/download/${tag}/${encodeURIComponent(name)}`,
  };
}

const mac = artifact(`_${version}_universal.app.tar.gz`);
const nsis = artifact(`_${version}_x64-setup.exe`);
const msi = artifact(`_${version}_x64_en-US.msi`);

// GitHub 자동 생성 변경 목록에서 PR 제목만 남김: "* feat: ... by @user in https://..." → "• feat: ..."
const notes = notesFile
  ? readFileSync(notesFile, "utf8")
      .split("\n")
      .filter((line) => line.startsWith("* "))
      .map((line) => `• ${line.slice(2).replace(/ by @\S+ in https:\/\/\S+$/, "")}`)
      .join("\n")
  : "";

const manifest = {
  version,
  notes,
  pub_date: new Date().toISOString().replace(/\.\d{3}Z$/, "Z"),
  platforms: {
    "darwin-aarch64": mac,
    "darwin-x86_64": mac,
    "windows-x86_64": nsis,
    "windows-x86_64-nsis": nsis,
    "windows-x86_64-msi": msi,
  },
};
writeFileSync(join(dir, "latest.json"), JSON.stringify(manifest, null, 2) + "\n");
console.log(JSON.stringify(manifest, null, 2));
