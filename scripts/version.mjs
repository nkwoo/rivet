#!/usr/bin/env node
// 앱 버전은 package.json, package-lock.json, tauri.conf.json, Cargo.toml, Cargo.lock 다섯 곳에 있습니다.
//
//   node scripts/version.mjs set 0.2.0     다섯 곳을 한 번에 바꿈
//   node scripts/version.mjs check         다섯 곳이 같은지 확인
//   node scripts/version.mjs check v0.2.0  태그와도 같은지 확인 (릴리스 워크플로)
import { readFileSync, writeFileSync } from "node:fs";
import { fileURLToPath } from "node:url";

const root = fileURLToPath(new URL("..", import.meta.url));
const path = (p) => root + p;

// MSI(WiX)는 숫자 세 자리 버전만 받으므로 프리릴리스 접미사는 쓰지 않습니다.
const SEMVER = /^(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)$/;

const read = (p) => readFileSync(path(p), "utf8");
const readJson = (p) => JSON.parse(read(p));
const writeJson = (p, value) => writeFileSync(path(p), JSON.stringify(value, null, 2) + "\n");

// [package] 표의 version 한 줄만 바꿉니다 (의존성의 version은 건드리지 않음).
const CARGO_TOML = /(\[package\][^[]*?\nversion = ")([^"]+)(")/;
// Cargo.lock에서 이 앱 패키지 항목
const CARGO_LOCK = /(\[\[package\]\]\nname = "rivet"\nversion = ")([^"]+)(")/;

function match(text, re, file) {
  const m = text.match(re);
  if (!m) throw new Error(`${file}: 버전 항목을 찾지 못했습니다`);
  return m[2];
}

function current() {
  const lock = readJson("package-lock.json");
  return {
    "package.json": readJson("package.json").version,
    "package-lock.json": lock.version,
    'package-lock.json packages[""]': lock.packages[""].version,
    "src-tauri/tauri.conf.json": readJson("src-tauri/tauri.conf.json").version,
    "src-tauri/Cargo.toml": match(read("src-tauri/Cargo.toml"), CARGO_TOML, "src-tauri/Cargo.toml"),
    "src-tauri/Cargo.lock": match(read("src-tauri/Cargo.lock"), CARGO_LOCK, "src-tauri/Cargo.lock"),
  };
}

function set(version) {
  if (!SEMVER.test(version)) throw new Error(`버전은 X.Y.Z 형식이어야 합니다: ${version}`);

  const pkg = readJson("package.json");
  pkg.version = version;
  writeJson("package.json", pkg);

  const lock = readJson("package-lock.json");
  lock.version = version;
  lock.packages[""].version = version;
  writeJson("package-lock.json", lock);

  const conf = readJson("src-tauri/tauri.conf.json");
  conf.version = version;
  writeJson("src-tauri/tauri.conf.json", conf);

  for (const [file, re] of [
    ["src-tauri/Cargo.toml", CARGO_TOML],
    ["src-tauri/Cargo.lock", CARGO_LOCK],
  ]) {
    const text = read(file);
    match(text, re, file);
    writeFileSync(path(file), text.replace(re, `$1${version}$3`));
  }

  check();
}

function check(tag) {
  const versions = current();
  const values = new Set(Object.values(versions));
  if (tag !== undefined) values.add(tag.replace(/^v/, ""));
  if (values.size !== 1) {
    for (const [file, v] of Object.entries(versions)) console.error(`  ${file}: ${v}`);
    if (tag !== undefined) console.error(`  태그: ${tag}`);
    throw new Error("버전이 서로 다릅니다");
  }
  const [version] = values;
  if (!SEMVER.test(version)) throw new Error(`버전은 X.Y.Z 형식이어야 합니다: ${version}`);
  if (tag !== undefined && tag !== `v${version}`) throw new Error(`태그는 v${version} 형식이어야 합니다: ${tag}`);
  console.log(version);
}

const [command, arg] = process.argv.slice(2);
try {
  if (command === "set" && arg) set(arg);
  else if (command === "check") check(arg);
  else {
    console.error("사용법: node scripts/version.mjs set <X.Y.Z> | check [vX.Y.Z]");
    process.exit(2);
  }
} catch (e) {
  console.error(e instanceof Error ? e.message : e);
  process.exit(1);
}
