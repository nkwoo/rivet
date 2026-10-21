import { api, errorMessage, type JobInfo } from "../lib/api.ts";
import { formatSize, fsErrorText, isFinished } from "./transfers.ts";
import { messages, useI18n } from "../i18n/index.ts";

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
  const m = messages();
  switch (s.kind) {
    case "queued":
      return { text: m.queue.queued, tone: "muted" };
    case "running": {
      const retry = job.attempt > 1 ? m.queue.retry(job.attempt - 1) : "";
      const speed = rate ? ` · ${formatSize(rate)}/s` : "";
      return { text: `${percent(job)}%${speed}${retry}`, tone: "" };
    }
    case "conflict":
      return { text: m.queue.conflict, tone: "warn" };
    case "done":
      return { text: m.queue.done, tone: "ok" };
    case "skipped":
      return { text: m.files.skipped[s.reason] ?? m.queue.skipped, tone: "muted" };
    case "failed":
      return { text: fsErrorText(s.code, s.message), tone: "warn" };
    case "cancelled":
      return { text: m.queue.cancelled, tone: "muted" };
  }
}

/** 이 세션의 전송 목록 (시안: 하단 큐) */
export function TransferQueue({ jobs, rates, onError }: TransferQueueProps) {
  const { m } = useI18n();
  const active = jobs.filter((j) => !isFinished(j));
  const running = jobs.filter((j) => j.state.kind === "running").length;
  const totalRate = jobs.reduce((sum, j) => sum + (j.state.kind === "running" ? (rates[j.id] ?? 0) : 0), 0);
  const hasFinished = jobs.some(isFinished);

  const run = (p: Promise<unknown>) => p.catch((e: unknown) => onError(errorMessage(e)));

  return (
    <section aria-label={m.queue.label} className="queue">
      <div className="queue-head">
        <span className="queue-title">{m.files.title}</span>
        <span className="mono muted queue-summary">
          {jobs.length === 0
            ? m.queue.empty
            : `${m.queue.summary(jobs.length, running, active.length)}${totalRate ? ` · ${formatSize(totalRate)}/s` : ""}`}
        </span>
        <button
          type="button"
          className="button button-secondary button-small queue-clear"
          disabled={!hasFinished}
          onClick={() => run(api.transferClearFinished())}
        >
          {m.queue.clearFinished}
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
                  aria-label={upload ? m.files.upload : m.files.download}
                  title={upload ? m.files.upload : m.files.download}
                >
                  {upload ? "↑" : "↓"}
                </span>
                <span className="queue-name" title={job.source}>
                  {job.name}
                </span>
                <span className="queue-target mono" title={job.target}>
                  {blocked ? m.queue.blocked : `→ ${job.target}`}
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
                      {m.queue.retryButton}
                    </button>
                  )}
                  {canCancel && (
                    <button
                      type="button"
                      className="button button-ghost button-tiny"
                      onClick={() => run(api.transferCancel(job.id))}
                    >
                      {m.common.cancel}
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
