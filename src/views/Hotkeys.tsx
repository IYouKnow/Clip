import { PageHeader } from "../components/ui/PageHeader";
import { Panel } from "../components/ui/Panel";

const SHORTCUTS = [
  { action: "Start or stop replay", keys: "Not yet set" },
  { action: "Save clip", keys: "Not yet set" },
  { action: "Show or hide window", keys: "Not yet set" },
];

export default function Hotkeys() {
  return (
    <>
      <PageHeader
        title="Hotkeys"
        description="Global shortcuts are planned but not wired up yet. Nothing on this page is active."
      />

      <div className="p-6">
        <Panel title="Planned shortcuts" bodyClassName="p-0">
          <ul className="divide-y divide-line">
            {SHORTCUTS.map((entry) => (
              <li
                key={entry.action}
                className="flex items-center justify-between gap-4 px-4 py-3"
              >
                <span className="text-sm">{entry.action}</span>
                <span className="flex items-center gap-3">
                  <kbd className="rounded-[6px] border border-line bg-elevated px-2 py-1 text-[12px] text-ink-muted">
                    {entry.keys}
                  </kbd>
                  <span className="rounded-full border border-line px-2 py-0.5 text-[11px] uppercase tracking-wider text-ink-faint">
                    Planned
                  </span>
                </span>
              </li>
            ))}
          </ul>
        </Panel>
      </div>
    </>
  );
}
