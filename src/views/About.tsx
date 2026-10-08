import { getVersion } from "@tauri-apps/api/app";
import { useEffect, useState } from "react";
import { Banner } from "../components/ui/Banner";
import { PageHeader } from "../components/ui/PageHeader";
import { Panel } from "../components/ui/Panel";
import { StatTile } from "../components/ui/StatTile";
import { formatMbps, type Status } from "../lib/api";

export default function About({ status }: { status: Status | null }) {
  const [version, setVersion] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    getVersion()
      .then(setVersion)
      .catch((caught) => setError(String(caught)));
  }, []);

  const encoders = status?.available_encoders ?? [];

  return (
    <>
      <PageHeader
        title="About"
        description="Build information and capture diagnostics for this installation."
      />

      <div className="flex flex-col gap-4 p-6">
        {error && <Banner tone="error">{error}</Banner>}

        <Panel title="Clipper23">
          <dl className="grid grid-cols-1 gap-3 sm:grid-cols-2">
            <div>
              <dt className="text-[11px] uppercase tracking-wider text-ink-faint">
                Version
              </dt>
              <dd className="mt-1 text-sm tabular-nums">{version ?? "…"}</dd>
            </div>
            <div>
              <dt className="text-[11px] uppercase tracking-wider text-ink-faint">
                Identifier
              </dt>
              <dd className="mt-1 text-sm">com.clipper23.app</dd>
            </div>
            <div className="sm:col-span-2">
              <dt className="text-[11px] uppercase tracking-wider text-ink-faint">
                Built with
              </dt>
              <dd className="mt-1 text-sm">Tauri 2, React 19, FFmpeg</dd>
            </div>
          </dl>
        </Panel>

        <Panel title="Capture diagnostics">
          <dl className="grid grid-cols-2 gap-3 sm:grid-cols-3 lg:grid-cols-4">
            <StatTile label="Encoder" value={status?.encoder ?? "—"} />
            <StatTile label="Pipeline" value={status?.pipeline ?? "—"} />
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
        </Panel>

        <Panel title="Available encoders">
          {encoders.length === 0 ? (
            <p className="text-[13px] text-ink-muted">
              No encoders reported yet. Start replay once to probe the system.
            </p>
          ) : (
            <ul className="flex flex-wrap gap-2">
              {encoders.map((name) => (
                <li
                  key={name}
                  className="rounded-[var(--radius-control)] border border-line bg-elevated px-2.5 py-1 text-[13px]"
                >
                  {name}
                </li>
              ))}
            </ul>
          )}
        </Panel>

        <Panel title="Storage">
          <p className="break-all text-[13px] text-ink-muted">
            {status?.clips_dir ?? "—"}
          </p>
        </Panel>
      </div>
    </>
  );
}
