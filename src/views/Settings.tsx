import { useEffect, useState } from "react";
import { api, type Settings, type Status } from "../lib/api";

type Props = {
  status: Status | null;
  onSaved: () => void;
};

export default function SettingsView({ status, onSaved }: Props) {
  const [settings, setSettings] = useState<Settings | null>(null);
  const [saved, setSaved] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    api
      .getSettings()
      .then(setSettings)
      .catch((caught) => setError(String(caught)));
  }, []);

  if (!settings) {
    return (
      <section className="card">
        <h2>Settings</h2>
        <p className="muted">{error ?? "Loading…"}</p>
      </section>
    );
  }

  function update(patch: Partial<Settings>) {
    setSettings((current) => (current ? { ...current, ...patch } : current));
    setSaved(false);
  }

  async function save() {
    if (!settings) return;
    try {
      await api.setSettings(settings);
      setSaved(true);
      setError(null);
      onSaved();
    } catch (caught) {
      setError(String(caught));
    }
  }

  const encoders = status?.available_encoders ?? [];

  return (
    <div className="stack">
      <section className="card">
        <h2>Capture</h2>

        <label className="field">
          <span>Replay buffer length</span>
          <div className="row">
            <input
              type="range"
              min={10}
              max={300}
              step={5}
              value={settings.buffer_seconds}
              onChange={(event) => update({ buffer_seconds: Number(event.target.value) })}
            />
            <output>{settings.buffer_seconds}s</output>
          </div>
        </label>

        <label className="field">
          <span>Frame rate</span>
          <select
            value={settings.fps}
            onChange={(event) => update({ fps: Number(event.target.value) })}
          >
            <option value={30}>30 fps</option>
            <option value={60}>60 fps</option>
          </select>
        </label>

        <label className="field">
          <span>Video bitrate</span>
          <div className="row">
            <input
              type="range"
              min={5}
              max={50}
              step={1}
              value={Math.round(settings.bitrate / 1_000_000)}
              onChange={(event) =>
                update({ bitrate: Number(event.target.value) * 1_000_000 })
              }
            />
            <output>{Math.round(settings.bitrate / 1_000_000)} Mbps</output>
          </div>
        </label>

        <label className="field">
          <span>Encoder</span>
          <select
            value={settings.encoder ?? ""}
            onChange={(event) => update({ encoder: event.target.value || null })}
          >
            <option value="">Auto (recommended)</option>
            {encoders.map((name) => (
              <option key={name} value={name}>
                {name}
              </option>
            ))}
          </select>
        </label>

        <div className="row">
          <button className="btn primary" onClick={save}>
            Save settings
          </button>
          {saved && <span className="muted">Saved</span>}
        </div>
        {error && <div className="banner error">{error}</div>}
      </section>

      <section className="card">
        <h2>Storage</h2>
        <p className="muted small">{status?.clips_dir ?? "—"}</p>
      </section>
    </div>
  );
}
