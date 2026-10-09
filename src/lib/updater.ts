import { relaunch } from "@tauri-apps/plugin-process";
import { check, type Update } from "@tauri-apps/plugin-updater";
import { useCallback, useRef, useState } from "react";

export type UpdaterPhase =
  | "idle"
  | "checking"
  | "available"
  | "downloading"
  | "ready"
  | "error";

export type Updater = {
  phase: UpdaterPhase;
  /** Version the running app would update to. */
  version: string | null;
  notes: string | null;
  /** Download progress as a whole percentage (0-100). */
  progress: number | null;
  error: string | null;
  check: (options?: { silent?: boolean }) => Promise<void>;
  install: () => Promise<void>;
};

/// Tracks the configured updater endpoint and drives download + install.
export function useUpdater(): Updater {
  const [phase, setPhase] = useState<UpdaterPhase>("idle");
  const [version, setVersion] = useState<string | null>(null);
  const [notes, setNotes] = useState<string | null>(null);
  const [progress, setProgress] = useState<number | null>(null);
  const [error, setError] = useState<string | null>(null);

  // The plugin resource isn't serialisable, so it lives in a ref, not state.
  const pending = useRef<Update | null>(null);

  const checkForUpdate = useCallback(async ({ silent = false } = {}) => {
    setPhase("checking");
    setError(null);
    try {
      const update = await check();
      pending.current = update;
      if (update) {
        setVersion(update.version);
        setNotes(update.body ?? null);
        setPhase("available");
      } else {
        setVersion(null);
        setNotes(null);
        setPhase("idle");
      }
    } catch (caught) {
      pending.current = null;
      // The launch check runs unattended; a missing release or no network
      // should stay invisible rather than surface an error banner.
      if (silent) {
        setPhase("idle");
      } else {
        setError(String(caught));
        setPhase("error");
      }
    }
  }, []);

  const install = useCallback(async () => {
    const update = pending.current;
    if (!update) return;

    setPhase("downloading");
    setProgress(0);
    setError(null);

    let downloaded = 0;
    let total = 0;
    let lastPercent = -1;

    try {
      await update.downloadAndInstall((event) => {
        if (event.event === "Started") {
          total = event.data.contentLength ?? 0;
        } else if (event.event === "Progress") {
          downloaded += event.data.chunkLength;
          if (total > 0) {
            const percent = Math.min(100, Math.floor((downloaded / total) * 100));
            // Only re-render on a whole-percent change; chunks arrive fast.
            if (percent !== lastPercent) {
              lastPercent = percent;
              setProgress(percent);
            }
          }
        }
      });
      setProgress(100);
      setPhase("ready");
      // On Windows the installer already exited and restarted the app, so this
      // only matters on other platforms.
      await relaunch();
    } catch (caught) {
      setError(String(caught));
      setPhase("error");
    }
  }, []);

  return {
    phase,
    version,
    notes,
    progress,
    error,
    check: checkForUpdate,
    install,
  };
}
