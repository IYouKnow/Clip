import { listen } from "@tauri-apps/api/event";
import { Film, LoaderCircle, Play, Save, Square, Trash2 } from "lucide-react";
import { useCallback, useEffect, useState } from "react";
import { CapturePreview } from "../components/CapturePreview";
import { ClipThumbnail } from "../components/ClipThumbnail";
import { Banner } from "../components/ui/Banner";
import { api, assetUrl, type Clip, type Status } from "../lib/api";
import { cx } from "../lib/cx";

/// Encoders that typically run on the CPU rather than a GPU block.
const SOFTWARE_ENCODERS = ["h264_mf", "libopenh264"];

const RECENT_LIMIT = 3;

type Props = {
  status: Status | null;
  pollError: string | null;
  onChanged: () => void;
};

export default function Home({ status, pollError, onChanged }: Props) {
  const [saveBusy, setSaveBusy] = useState(false);
  const [replayBusy, setReplayBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [clips, setClips] = useState<Clip[]>([]);
  const [now, setNow] = useState(() => Date.now());

  const replaying = status?.replaying ?? false;
  const replayStartedMs = status?.replay_started_ms ?? null;
  const bufferSeconds = status?.buffer_seconds ?? 0;

  // Tick while recording so the elapsed timer advances between status polls.
  useEffect(() => {
    if (!replaying) return;
    setNow(Date.now());
    const timer = setInterval(() => setNow(Date.now()), 1000);
    return () => clearInterval(timer);
  }, [replaying]);

  const recordedSeconds =
    replaying && replayStartedMs
      ? Math.max(0, Math.floor((now - replayStartedMs) / 1000))
      : 0;
  const softwareEncoder = status?.encoder
    ? SOFTWARE_ENCODERS.includes(status.encoder)
    : false;

  const loadClips = useCallback(async () => {
    try {
      const all = await api.listClips();
      setClips([...all].sort((a, b) => b.modified_ms - a.modified_ms).slice(0, RECENT_LIMIT));
    } catch {
      setClips([]);
    }
  }, []);

  useEffect(() => {
    loadClips();
  }, [loadClips]);

  // Any save path — the button, the global hotkey, or the tray — emits this,
  // so recents update without needing to leave and re-enter the view.
  useEffect(() => {
    let unlisten: (() => void) | undefined;
    listen("clips-changed", () => {
      loadClips();
    })
      .then((stop) => {
        unlisten = stop;
      })
      .catch(() => {});
    return () => unlisten?.();
  }, [loadClips]);

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
      await api.saveClip();
      onChanged();
    } catch (caught) {
      setError(String(caught));
    } finally {
      setSaveBusy(false);
    }
  }

  /// Moves a recent clip to the Recycle Bin; the backend then emits
  /// `clips-changed`, which reloads the list.
  async function remove(clip: Clip) {
    try {
      await api.deleteClip(clip.path);
    } catch (caught) {
      setError(String(caught));
    }
  }

  const shownError = error ?? pollError;

  return (
    <div className="flex flex-1 flex-col">
      <div className="mx-auto my-auto flex w-full max-w-xl flex-col gap-5 px-6 py-8">
        {shownError && <Banner tone="error">{shownError}</Banner>}

        {softwareEncoder && (
          <Banner tone="warn">
            <strong>{status?.encoder}</strong> may be a software encoder and can make the
            whole PC feel slow. Install your GPU driver so a hardware encoder can be used.
          </Banner>
        )}

        <CapturePreview
          replaying={replaying}
          bufferSeconds={bufferSeconds}
          recordedSeconds={recordedSeconds}
        />

        <div className="flex flex-col items-center gap-2.5">
          <div className="flex items-center justify-center gap-3">
            <button
              type="button"
              aria-label={replaying ? "Stop replay" : "Start replay"}
              title={replaying ? "Stop replay" : "Start replay"}
              disabled={replayBusy}
              onClick={runReplay}
              className={cx(
                "grid size-14 cursor-pointer place-items-center rounded-full border transition duration-150 disabled:pointer-events-none disabled:opacity-60",
                replaying
                  ? "border-line bg-surface text-ink hover:bg-elevated"
                  : "border-transparent bg-elevated text-ink hover:bg-line",
              )}
            >
              {replayBusy ? (
                <LoaderCircle className="size-5 animate-spin" />
              ) : replaying ? (
                <Square className="size-4" />
              ) : (
                <Play className="size-5" />
              )}
            </button>

            <button
              type="button"
              disabled={saveBusy || !replaying}
              onClick={save}
              className="save-glow inline-flex h-14 cursor-pointer items-center justify-center gap-2.5 rounded-full bg-accent px-8 text-[15px] font-semibold text-accent-ink transition duration-150 hover:brightness-105 active:scale-[0.99] disabled:pointer-events-none disabled:opacity-45"
            >
              {saveBusy ? (
                <LoaderCircle className="size-5 animate-spin" />
              ) : (
                <Save className="size-5" />
              )}
              Save clip
            </button>
          </div>

          <p className="text-center text-[13px] text-ink-muted">
            {replaying
              ? `Saves the last ${bufferSeconds}s to your clips folder`
              : "Start replay to keep a rolling buffer"}
          </p>
        </div>

        <section className="flex flex-col gap-2">
          <h2 className="text-[12px] font-semibold uppercase tracking-wider text-ink-muted">
            Recent clips
          </h2>
          {clips.length === 0 ? (
            <p className="rounded-[var(--radius-panel)] border border-dashed border-line px-3.5 py-4 text-center text-[13px] text-ink-faint">
              No clips yet — saved clips show up here.
            </p>
          ) : (
            <div className="grid grid-cols-3 gap-2">
              {clips.map((clip) => (
                <div
                  key={clip.path}
                  className="group relative aspect-video overflow-hidden rounded-[var(--radius-control)] border border-line bg-black transition-colors hover:border-line-strong"
                >
                  <button
                    type="button"
                    onClick={() => api.openClip(clip.path)}
                    title={clip.name}
                    aria-label={`Open ${clip.name}`}
                    className="absolute inset-0 cursor-pointer"
                  >
                    <span className="absolute inset-0 grid place-items-center text-ink-faint">
                      <Film className="size-5" />
                    </span>
                    <ClipThumbnail src={assetUrl(clip.path)} />
                    <span className="absolute inset-0 grid place-items-center bg-black/0 opacity-0 transition duration-150 group-hover:bg-black/35 group-hover:opacity-100">
                      <Play className="size-6 text-white" />
                    </span>
                  </button>
                  <button
                    type="button"
                    onClick={() => remove(clip)}
                    title="Delete clip"
                    aria-label={`Delete ${clip.name}`}
                    className="invisible absolute right-1.5 top-1.5 grid size-8 cursor-pointer place-items-center rounded-full border border-white/15 bg-black/60 text-white opacity-0 backdrop-blur transition duration-150 hover:bg-rec group-hover:visible group-hover:opacity-100 group-focus-within:visible group-focus-within:opacity-100"
                  >
                    <Trash2 className="size-4" />
                  </button>
                </div>
              ))}
            </div>
          )}
        </section>
      </div>
    </div>
  );
}
