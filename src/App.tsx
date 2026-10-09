import { useCallback, useEffect, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { AppShell } from "./components/AppShell";
import { Sidebar } from "./components/Sidebar";
import { api, type Status } from "./lib/api";
import type { View } from "./lib/nav";
import { useTheme } from "./lib/theme";
import { useUpdater } from "./lib/updater";
import Home from "./views/Home";
import Hotkeys from "./views/Hotkeys";
import Library from "./views/Library";
import SettingsView from "./views/Settings";

export default function App() {
  const { theme, setTheme } = useTheme();
  const [view, setView] = useState<View>("home");
  const [status, setStatus] = useState<Status | null>(null);
  const [error, setError] = useState<string | null>(null);
  const updater = useUpdater();

  const refresh = useCallback(async () => {
    try {
      setStatus(await api.getStatus());
      setError(null);
    } catch (caught) {
      setError(String(caught));
    }
  }, []);

  useEffect(() => {
    refresh();
  }, [refresh]);

  useEffect(() => {
    const timer = setInterval(refresh, 1000);
    return () => clearInterval(timer);
  }, [refresh]);

  // The tray can ask the UI to switch views (e.g. "Open Library").
  useEffect(() => {
    let unlisten: (() => void) | undefined;
    listen<string>("navigate", (event) => setView(event.payload as View))
      .then((stop) => {
        unlisten = stop;
      })
      .catch(() => {});
    return () => unlisten?.();
  }, []);

  // Check for a new release once at startup; the result lights up Settings.
  useEffect(() => {
    updater.check({ silent: true });
  }, [updater.check]);

  return (
    <AppShell
      sidebar={
        <Sidebar
          view={view}
          onSelect={setView}
          theme={theme}
          onThemeChange={setTheme}
          replaying={status?.replaying ?? false}
        />
      }
    >
      {view === "home" && (
        <Home status={status} pollError={error} onChanged={refresh} />
      )}
      {view === "library" && <Library />}
      {view === "settings" && (
        <SettingsView
          status={status}
          theme={theme}
          onThemeChange={setTheme}
          onSaved={refresh}
          updater={updater}
        />
      )}
      {view === "hotkeys" && <Hotkeys />}
    </AppShell>
  );
}
