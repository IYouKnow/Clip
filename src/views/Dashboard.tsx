import { Play, Save, Square } from "lucide-react";
import { useState } from "react";
import { Banner } from "../components/ui/Banner";
import { Button } from "../components/ui/Button";
import { PageHeader } from "../components/ui/PageHeader";
import { Panel } from "../components/ui/Panel";
import { StatTile } from "../components/ui/StatTile";
import { api, formatBytes, formatDate, formatMbps, type Clip, type Status } from "../lib/api";

/// Encoders that typically run on the CPU rather than a GPU block.
const SOFTWARE_ENCODERS = ["h264_mf", "libopenh264"];

type Props = {
  status: Status | null;
  pollError: string | null;
  onChanged: () => void;
};

export default function Dashboard({ status, pollError, onChanged }: Props) {
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

  const shownError = error ?? pollError;

  return (
    <>
      <PageHeader
        title="Dashboard"
        description="Keep a rolling buffer of your screen in memory and save the last few seconds at any time."
      >
        <Button
          variant={replaying ? "danger" : "primary"}
          loading={busy}
          icon={replaying ? <Square className="size-3.5" /> : <Play className="size-4" />}
          onClick={() => run(replaying ? api.stopReplay : api.startReplay)}
        >
          {replaying ? "Stop replay" : "Start replay"}
        </Button>
        <Button
          disabled={busy || !replaying}
          icon={<Save className="size-4" />}
          onClick={() =>
            run(async () => {
              setLastClip(await api.saveClip());
            })
          }
        >
          Save clip
        </Button>
      </PageHeader>

      <div className="flex flex-col gap-4 p-6">
        {shownError && <Banner tone="error">{shownError}</Banner>}

        {status?.encoder && SOFTWARE_ENCODERS.includes(status.encoder) && (
          <Banner tone="warn">
            Using <strong>{status.encoder}</strong>, which may be a software encoder. That
            uses a lot of CPU and can make the whole PC feel slow. If your GPU supports
            it, check that its driver is installed so a hardware encoder can be used.
          </Banner>
        )}

        <Panel>
          <div className="flex items-center gap-3">
            <span
              aria-hidden="true"
              className={
                replaying
                  ? "size-2.5 shrink-0 rounded-full bg-rec animate-[rec-pulse_1.6s_ease-in-out_infinite]"
                  : "size-2.5 shrink-0 rounded-full bg-ink-faint"
              }
            />
            <div>
              <h2 className="text-lg font-semibold">
                {replaying ? "Recording" : "Replay is off"}
              </h2>
              <p className="mt-0.5 text-[13px] text-ink-muted">
                {replaying
                  ? `Keeping the last ${status?.buffer_seconds ?? 0}s in memory.`
                  : "Start replay to keep a rolling buffer of your screen."}
              </p>
            </div>
          </div>

          <dl className="mt-4 grid grid-cols-2 gap-3 sm:grid-cols-3 lg:grid-cols-4">
            <StatTile
              label="Encoder"
              value={status?.encoder ?? (replaying ? "starting…" : "—")}
            />
            <StatTile
              label="Pipeline"
              value={status?.pipeline ?? (replaying ? "starting…" : "—")}
            />
            <StatTile
              label="Capture"
              value={status ? `${status.fps} fps` : "—"}
              hint={status ? formatMbps(status.bitrate) : undefined}
            />
            <StatTile label="Frames captured" value={status?.frames ?? 0} />
            <StatTile label="Packets buffered" value={status?.packets ?? 0} />
            <StatTile label="Frames skipped" value={status?.dropped ?? 0} />
            <StatTile label="Idle (no change)" value={status?.idle ?? 0} />
          </dl>

          {status?.pipeline === "zero-copy gpu" && (
            <p className="mt-4 text-[13px] text-ink-muted">
              Zero-copy GPU pipeline: captured frames stay on the GPU all the way to the
              encoder, so nothing full-frame crosses the CPU.
            </p>
          )}
          {status && (
            <p className="mt-2 text-[12px] text-ink-faint">
              Clips are saved to {status.clips_dir}
            </p>
          )}
        </Panel>

        {lastClip && (
          <Panel title="Last saved clip">
            <div className="flex items-center justify-between gap-4">
              <div className="min-w-0">
                <div className="truncate text-sm font-medium" title={lastClip.name}>
                  {lastClip.name}
                </div>
                <div className="mt-0.5 text-[12px] text-ink-muted">
                  {formatBytes(lastClip.size_bytes)} · {formatDate(lastClip.modified_ms)}
                </div>
              </div>
              <Button size="sm" onClick={() => api.openClip(lastClip.path)}>
                Open
              </Button>
            </div>
          </Panel>
        )}
      </div>
    </>
  );
}
