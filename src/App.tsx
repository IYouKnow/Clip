import { useCallback, useEffect, useState } from "react";
import { AppShell } from "./components/AppShell";
import { Sidebar } from "./components/Sidebar";
import { api, type Status } from "./lib/api";
import type { View } from "./lib/nav";
import { useTheme } from "./lib/theme";
import About from "./views/About";
import Dashboard from "./views/Dashboard";
import Hotkeys from "./views/Hotkeys";
import Library from "./views/Library";
import SettingsView from "./views/Settings";

export default function App() {
  const { theme, setTheme } = useTheme();
  const [view, setView] = useState<View>("dashboard");
  const [status, setStatus] = useState<Status | null>(null);
  const [error, setError] = useState<string | null>(null);

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
      {view === "dashboard" && (
        <Dashboard status={status} pollError={error} onChanged={refresh} />
      )}
      {view === "library" && <Library />}
      {view === "settings" && (
        <SettingsView
          status={status}
          theme={theme}
          onThemeChange={setTheme}
          onSaved={refresh}
        />
      )}
      {view === "hotkeys" && <Hotkeys />}
      {view === "about" && <About status={status} />}
    </AppShell>
  );
}
