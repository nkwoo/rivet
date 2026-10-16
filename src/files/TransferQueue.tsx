import { api, errorMessage, type JobInfo } from "../lib/api.ts";
import { formatSize, fsErrorText, isFinished, SKIP_TEXT } from "./transfers.ts";

interface TransferQueueProps {
  jobs: JobInfo[];
  rates: Record<string, number>;
  onError: (message: string) => void;
}

function percent(job: JobInfo): number {
  if (job.state.kind === "done") return 100;
  if (job.size === 0) return 0;
  return Math.min(100, Math.floor((job.transferred / job.size) * 100));
}

function status(job: JobInfo, rate: number | undefined): { text: string; tone: string } {
  const s = job.state;
  switch (s.kind) {
    case "queued":
      return { text: "대기 중", tone: "muted" };
    case "running": {
      const retry = job.attempt > 1 ? ` · 재시도 ${job.attempt - 1}` : "";
      const speed = rate ? ` · ${formatSize(rate)}/s` : "";
      return { text: `${percent(job)}%${speed}${retry}`, tone: "" };
    }
    case "conflict":
      return { text: "같은 이름 있음 · 확인 필요", tone: "warn" };
    case "done":
      return { text: "완료", tone: "ok" };
    case "skipped":
      return { text: SKIP_TEXT[s.reason] ?? "건너뜀", tone: "muted" };
    case "failed":
      return { text: fsErrorText(s.code, s.message), tone: "warn" };
    case "cancelled":
      return { text: "취소됨", tone: "muted" };
  }
}

/** 이 세션의 전송 목록 (시안: 하단 큐) */
export function TransferQueue({ jobs, rates, onError }: TransferQueueProps) {
  const active = jobs.filter((j) => !isFinished(j));
  const running = jobs.filter((j) => j.state.kind === "running").length;
  const totalRate = jobs.reduce((sum, j) => sum + (j.state.kind === "running" ? (rates[j.id] ?? 0) : 0), 0);
  const hasFinished = jobs.some(isFinished);

  const run = (p: Promise<unknown>) => p.catch((e: unknown) => onError(errorMessage(e)));

  return (
    <section aria-label="전송 큐" className="queue">
      <div className="queue-head">
        <span className="queue-title">전송</span>
        <span className="mono muted queue-summary">
          {jobs.length === 0
            ? "파일을 끌어다 놓거나 업로드/다운로드 버튼을 누르세요"
            : `${jobs.length}개 · 진행 ${running} · 남음 ${active.length}${totalRate ? ` · ${formatSize(totalRate)}/s` : ""}`}
        </span>
        <button
          type="button"
          className="button button-secondary button-small queue-clear"
          disabled={!hasFinished}
          onClick={() => run(api.transferClearFinished())}
        >
          완료 항목 지우기
        </button>
      </div>
      {jobs.length > 0 && (
        <ul className="queue-list">
          {jobs.map((job) => {
            const { text, tone } = status(job, rates[job.id]);
            const upload = job.direction === "upload";
            const canRetry =
              (job.state.kind === "failed" && job.state.retryable) || job.state.kind === "cancelled";
            const canCancel = !isFinished(job);
            const blocked = job.state.kind === "failed" && !job.state.retryable;
            return (
              <li key={job.id} className="queue-row">
                <span
                  className={`queue-dir mono ${upload ? "is-up" : "is-down"}`}
                  aria-label={upload ? "업로드" : "다운로드"}
                  title={upload ? "업로드" : "다운로드"}
                >
                  {upload ? "↑" : "↓"}
                </span>
                <span className="queue-name" title={job.source}>
                  {job.name}
                </span>
                <span className="queue-target mono" title={job.target}>
                  {blocked ? "대상 폴더 밖을 가리키거나 사용할 수 없는 이름" : `→ ${job.target}`}
                </span>
                <span className="queue-bar" aria-hidden="true">
                  <span
                    className={`queue-fill ${job.state.kind === "done" ? "is-done" : upload ? "is-up" : "is-down"}`}
                    style={{ width: `${percent(job)}%` }}
                  />
                </span>
                <span className={`queue-status ${tone && `tone-${tone}`}`}>
                  <span className="queue-status-text" title={text}>
                    {text}
                  </span>
                  {canRetry && (
                    <button
                      type="button"
                      className="button button-ghost button-tiny"
                      onClick={() => run(api.transferRetry(job.id))}
                    >
                      다시 시도
                    </button>
                  )}
                  {canCancel && (
                    <button
                      type="button"
                      className="button button-ghost button-tiny"
                      onClick={() => run(api.transferCancel(job.id))}
                    >
                      취소
                    </button>
                  )}
                </span>
              </li>
            );
          })}
        </ul>
      )}
    </section>
  );
}
