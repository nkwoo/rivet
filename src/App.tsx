import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { RivetMark } from "./RivetMark.tsx";

interface AppInfo {
  name: string;
  version: string;
}

export default function App() {
  const [info, setInfo] = useState<AppInfo | null>(null);

  useEffect(() => {
    invoke<AppInfo>("app_info")
      .then(setInfo)
      .catch(() => setInfo(null));
  }, []);

  return (
    <div className="shell">
      <main className="welcome">
        <div className="lockup">
          <RivetMark size={72} />
          <span className="wordmark">Rivet</span>
        </div>
        <p className="tagline">SSH, 파일 전송, 원격 데스크톱을 하나의 창에.</p>
      </main>
      <footer className="statusbar">
        <span>{info ? `${info.name} ${info.version}` : "Rivet"}</span>
        <span className="status-muted">연결 없음</span>
      </footer>
    </div>
  );
}
