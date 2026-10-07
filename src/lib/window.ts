import { useEffect, useState } from "react";
import { getCurrentWindow, type Window } from "@tauri-apps/api/window";

/// The Tauri window handle, or null when rendered outside Tauri (e.g. `vite dev`).
export const appWindow: Window | null = (() => {
  try {
    return getCurrentWindow();
  } catch {
    return null;
  }
})();

/// Tracks whether the window is maximized so the control can swap its glyph.
export function useMaximized(): boolean {
  const [maximized, setMaximized] = useState(false);

  useEffect(() => {
    if (!appWindow) return;

    let disposed = false;
    let unlisten: (() => void) | undefined;

    const sync = () => {
      appWindow.isMaximized().then((value) => {
        if (!disposed) setMaximized(value);
      });
    };

    sync();
    appWindow.onResized(sync).then((stop) => {
      if (disposed) stop();
      else unlisten = stop;
    });

    return () => {
      disposed = true;
      unlisten?.();
    };
  }, []);

  return maximized;
}
