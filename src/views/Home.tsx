import { Film, LoaderCircle, Play, Save, Square } from "lucide-react";
import { useState } from "react";
import { CapturePreview } from "../components/CapturePreview";
import { Banner } from "../components/ui/Banner";
import { Button } from "../components/ui/Button";
import { api, formatBytes, formatDate, type Clip, type Status } from "../lib/api";

/// Encoders that typically run on the CPU rather than a GPU block.
const SOFTWARE_ENCODERS = ["h264_mf", "libopenh264"];

type Props = {
  status: Status | null;
  pollError: string | null;
  onChanged: () => void;
};

export default function Home({ status, pollError, onChanged }: Props) {
  const [saveBusy, setSaveBusy] = useState(false);
  const [replayBusy, setReplayBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [lastClip, setLastClip] = useState<Clip | null>(null);

  const replaying = status?.replaying ?? false;
  const bufferSeconds = status?.buffer_seconds ?? 0;
  const softwareEncoder = status?.encoder
    ? SOFTWARE_ENCODERS.includes(status.encoder)
    : false;

  async function runReplay() {
    setReplayBusy(true);
    setError(null);
    try {
      await (replaying ? api.stopReplay() : api.startReplay());
      onChanged();
    } catch (caught) {
      setError(String(caught));
    } finally {
      setReplayBusy(false);
    }
  }

  async function save() {
    setSaveBusy(true);
    setError(null);
    try {
      setLastClip(await api.saveClip());
      onChanged();
    } catch (caught) {
      setError(String(caught));
    } finally {
      setSaveBusy(false);
    }
  }

  const shownError = error ?? pollError;
  const heroBusy = replaying ? saveBusy : replayBusy;

  return (
    <div className="flex flex-1 flex-col">
      <div className="mx-auto my-auto flex w-full max-w-xl flex-col gap-4 px-6 py-8">
        {shownError && <Banner tone="error">{shownError}</Banner>}

        {softwareEncoder && (
          <Banner tone="warn">
            <strong>{status?.encoder}</strong> may be a software encoder and can make the
            whole PC feel slow. Install your GPU driver so a hardware encoder can be used.
          </Banner>
        )}

        <CapturePreview replaying={replaying} bufferSeconds={bufferSeconds} />

        <div className="flex flex-col items-center gap-2.5">
          <button
            type="button"
            disabled={heroBusy}
            onClick={replaying ? save : runReplay}
            className="save-glow inline-flex h-14 cursor-pointer items-center justify-center gap-2.5 rounded-full bg-accent px-9 text-[15px] font-semibold text-accent-ink transition duration-150 hover:brightness-105 active:scale-[0.99] disabled:pointer-events-none disabled:opacity-70"
          >
            {heroBusy ? (
              <LoaderCircle className="size-5 animate-spin" />
            ) : replaying ? (
              <Save className="size-5" />
            ) : (
              <Play className="size-5" />
            )}
            {replaying ? "Save clip" : "Start replay"}
          </button>
          <p className="text-center text-[13px] text-ink-muted">
            {replaying
              ? `Saves the last ${bufferSeconds}s to your clips folder`
              : "Start a rolling buffer and save the last few seconds anytime"}
          </p>
        </div>

        {replaying && (
          <div className="flex justify-center">
            <Button
              variant="ghost"
              size="sm"
              loading={replayBusy}
              icon={<Square className="size-3" />}
              onClick={runReplay}
            >
              Stop replay
            </Button>
          </div>
        )}

        {lastClip && (
          <button
            type="button"
            onClick={() => api.openClip(lastClip.path)}
            className="mx-auto flex w-full max-w-sm cursor-pointer items-center gap-3 rounded-[var(--radius-panel)] border border-line bg-surface px-3.5 py-2.5 text-left transition-colors hover:bg-elevated"
          >
            <span className="grid size-8 shrink-0 place-items-center rounded-[var(--radius-control)] bg-rec/15 text-rec">
              <Film className="size-4" />
            </span>
            <span className="min-w-0 flex-1">
              <span className="block truncate text-[13px] font-medium">
                Saved {lastClip.name}
              </span>
              <span className="block text-[12px] text-ink-muted">
                {formatBytes(lastClip.size_bytes)} · {formatDate(lastClip.modified_ms)}
              </span>
            </span>
            <span className="shrink-0 text-[12px] font-medium text-ink-muted">Open</span>
          </button>
        )}
      </div>
    </div>
  );
}
