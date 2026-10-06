import { useState } from "react";
import { api, formatBytes, formatDate, type Clip, type Status } from "../lib/api";

type Props = {
  status: Status | null;
  onChanged: () => void;
};

/// Encoders that typically run on the CPU rather than a GPU block.
const SOFTWARE_ENCODERS = ["h264_mf", "libx264"];

export default function Home({ status, onChanged }: Props) {
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [lastClip, setLastClip] = useState<Clip | null>(null);

  const replaying = status?.replaying ?? false;

  async function run(action: () => Promise<unknown>) {
    setBusy(true);
    setError(null);
    try {
      await action();
      onChanged();
    } catch (caught) {
      setError(String(caught));
    } finally {
      setBusy(false);
    }
  }

  return (
    <div className="stack">
      <section className="card hero">
        <div className="hero-main">
          <h1>{replaying ? "Recording" : "Replay is off"}</h1>
          <p className="muted">
            {replaying
              ? `Keeping the last ${status?.buffer_seconds}s in memory.`
              : "Start replay to keep a rolling buffer of your screen."}
          </p>
        </div>
        <div className="hero-actions">
          <button
            className={replaying ? "btn danger" : "btn primary"}
            disabled={busy}
            onClick={() => run(replaying ? api.stopReplay : api.startReplay)}
          >
            {replaying ? "Stop replay" : "Start replay"}
          </button>
          <button
            className="btn"
            disabled={busy || !replaying}
            onClick={() =>
              run(async () => {
                setLastClip(await api.saveClip());
              })
            }
          >
            Save clip
          </button>
        </div>
      </section>

      {error && <div className="banner error">{error}</div>}

      {status?.encoder && SOFTWARE_ENCODERS.includes(status.encoder) && (
        <div className="banner warn">
          Using <strong>{status.encoder}</strong>, which may be a software encoder — that
          uses a lot of CPU and can make the whole PC feel slow. If your GPU supports it,
          check that its driver is installed so a hardware encoder can be used.
        </div>
      )}

      <section className="card">
        <h2>Status</h2>
        <dl className="stats">
          <div>
            <dt>Encoder</dt>
            <dd>{status?.encoder ?? (replaying ? "starting…" : "—")}</dd>
          </div>
          <div>
            <dt>Frames captured</dt>
            <dd>{status?.frames ?? 0}</dd>
          </div>
          <div>
            <dt>Packets buffered</dt>
            <dd>{status?.packets ?? 0}</dd>
          </div>
          <div>
            <dt>Frames skipped</dt>
            <dd>{status?.dropped ?? 0}</dd>
          </div>
          <div>
            <dt>Capture</dt>
            <dd>
              {status ? `${status.fps} fps · ${(status.bitrate / 1_000_000).toFixed(0)} Mbps` : "—"}
            </dd>
          </div>
        </dl>
        {status && <p className="muted small">Clips are saved to {status.clips_dir}</p>}
      </section>

      {lastClip && (
        <section className="card">
          <h2>Last saved clip</h2>
          <div className="row">
            <div>
              <div className="clip-name">{lastClip.name}</div>
              <div className="muted small">
                {formatBytes(lastClip.size_bytes)} · {formatDate(lastClip.modified_ms)}
              </div>
            </div>
            <button className="btn" onClick={() => api.openClip(lastClip.path)}>
              Open
            </button>
          </div>
        </section>
      )}
    </div>
  );
}
