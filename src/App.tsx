import { useCallback, useEffect, useState } from "react";
import { api, type Status } from "./lib/api";
import Home from "./views/Home";
import Library from "./views/Library";
import SettingsView from "./views/Settings";
import "./App.css";

type Tab = "home" | "library" | "settings";

const TABS: { id: Tab; label: string }[] = [
  { id: "home", label: "Replay" },
  { id: "library", label: "Library" },
  { id: "settings", label: "Settings" },
];

export default function App() {
  const [tab, setTab] = useState<Tab>("home");
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
    <div className="app">
      <header className="topbar">
        <div className="brand">
          <span className="brand-dot" data-on={status?.replaying ?? false} />
          Clipper23
        </div>
        <nav className="tabs">
          {TABS.map((entry) => (
            <button
              key={entry.id}
              className={tab === entry.id ? "tab active" : "tab"}
              onClick={() => setTab(entry.id)}
            >
              {entry.label}
            </button>
          ))}
        </nav>
      </header>

      {error && <div className="banner error">{error}</div>}

      <main className="content">
        {tab === "home" && <Home status={status} onChanged={refresh} />}
        {tab === "library" && <Library />}
        {tab === "settings" && <SettingsView status={status} onSaved={refresh} />}
      </main>
    </div>
  );
}
