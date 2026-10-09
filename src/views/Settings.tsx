import { Download, FolderOpen, RefreshCw } from "lucide-react";
import { useEffect, useState } from "react";
import { ThemeToggle } from "../components/ThemeToggle";
import { Banner } from "../components/ui/Banner";
import { Button } from "../components/ui/Button";
import { PageHeader } from "../components/ui/PageHeader";
import { Panel } from "../components/ui/Panel";
import { Select } from "../components/ui/Select";
import { Slider } from "../components/ui/Slider";
import { api, type AudioSource, type Settings, type Status } from "../lib/api";
import type { Theme } from "../lib/theme";
import type { Updater } from "../lib/updater";

type Props = {
  status: Status | null;
  theme: Theme;
  onThemeChange: (theme: Theme) => void;
  onSaved: () => void;
  updater: Updater;
};

export default function SettingsView({
  status,
  theme,
  onThemeChange,
  onSaved,
  updater,
}: Props) {
  const [settings, setSettings] = useState<Settings | null>(null);
  const [version, setVersion] = useState<string | null>(null);
  const [saved, setSaved] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    api
      .getSettings()
      .then(setSettings)
      .catch((caught) => setError(String(caught)));
    api
      .getVersion()
      .then(setVersion)
      .catch(() => {});
  }, []);

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
  const clipsDir = status?.clips_dir;

  return (
    <>
      <PageHeader title="Settings" description="Capture, appearance, and where clips land." />

      <div className="flex max-w-2xl flex-col gap-4 p-6">
        {error && <Banner tone="error">{error}</Banner>}

        <Panel title="Capture">
          {!settings ? (
            <p className="text-[13px] text-ink-muted">Loading…</p>
          ) : (
            <div className="flex flex-col gap-5">
              <Slider
                label="Replay buffer length"
                min={10}
                max={300}
                step={5}
                value={settings.buffer_seconds}
                onChange={(value) => update({ buffer_seconds: value })}
                format={(value) => `${value}s`}
              />
              <Select
                label="Frame rate"
                value={String(settings.fps)}
                onChange={(value) => update({ fps: Number(value) })}
              >
                <option value="30">30 fps</option>
                <option value="60">60 fps</option>
              </Select>
              <Slider
                label="Video bitrate"
                min={5}
                max={50}
                step={1}
                value={Math.round(settings.bitrate / 1_000_000)}
                onChange={(value) => update({ bitrate: value * 1_000_000 })}
                format={(value) => `${value} Mbps`}
              />
              <Select
                label="Encoder"
                value={settings.encoder ?? ""}
                onChange={(value) => update({ encoder: value || null })}
              >
                <option value="">Auto (recommended)</option>
                {encoders.map((name) => (
                  <option key={name} value={name}>
                    {name}
                  </option>
                ))}
              </Select>

              <Select
                label="Audio source"
                value={settings.audio_source}
                onChange={(value) => update({ audio_source: value as AudioSource })}
              >
                <option value="off">Off</option>
                <option value="system">System audio</option>
                <option value="microphone">Microphone</option>
                <option value="both">System + microphone</option>
              </Select>

              <div className="flex items-center gap-3">
                <Button variant="primary" onClick={save}>
                  Save settings
                </Button>
                {saved && <span className="text-[13px] text-ink-muted">Saved</span>}
              </div>
            </div>
          )}
        </Panel>

        <Panel title="Appearance">
          <div className="flex flex-col gap-2">
            <span className="text-[13px] text-ink-muted">Theme</span>
            <div className="max-w-[220px]">
              <ThemeToggle theme={theme} onChange={onThemeChange} />
            </div>
            <p className="text-[12px] text-ink-faint">System follows your Windows theme.</p>
          </div>
        </Panel>

        <Panel title="Storage">
          <div className="flex items-center justify-between gap-4">
            <p className="break-all text-[13px] text-ink-muted">{clipsDir ?? "—"}</p>
            <Button
              size="sm"
              disabled={!clipsDir}
              icon={<FolderOpen className="size-3.5" />}
              onClick={() => clipsDir && api.openClip(clipsDir)}
            >
              Open folder
            </Button>
          </div>
        </Panel>

        <Panel title="Updates">
          <div className="flex flex-col gap-3">
            <div className="flex items-center justify-between gap-4">
              <p className="text-[13px] text-ink-muted">
                Trace {version ?? "…"}
              </p>
              <Button
                size="sm"
                icon={<RefreshCw className="size-3.5" />}
                loading={updater.phase === "checking"}
                onClick={() => updater.check()}
              >
                Check for updates
              </Button>
            </div>

            {(updater.phase === "available" ||
              updater.phase === "downloading" ||
              updater.phase === "ready") && (
              <Banner tone="info">
                <div className="flex flex-col gap-2">
                  <span>Version {updater.version} is available.</span>
                  {updater.notes && (
                    <span className="line-clamp-3 text-ink-muted">{updater.notes}</span>
                  )}
                  <div>
                    <Button
                      variant="primary"
                      size="sm"
                      icon={<Download className="size-3.5" />}
                      loading={updater.phase === "downloading"}
                      disabled={updater.phase !== "available"}
                      onClick={() => updater.install()}
                    >
                      {updater.phase === "downloading"
                        ? `Updating… ${updater.progress ?? 0}%`
                        : "Update & restart"}
                    </Button>
                  </div>
                </div>
              </Banner>
            )}

            {updater.phase === "error" && <Banner tone="error">{updater.error}</Banner>}
          </div>
        </Panel>
      </div>
    </>
  );
}
