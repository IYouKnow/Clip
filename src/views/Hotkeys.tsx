import { Keyboard, X } from "lucide-react";
import { useEffect, useState } from "react";
import { Banner } from "../components/ui/Banner";
import { Button } from "../components/ui/Button";
import { PageHeader } from "../components/ui/PageHeader";
import { Panel } from "../components/ui/Panel";
import { api, type Hotkeys } from "../lib/api";
import { cx } from "../lib/cx";

type Field = keyof Hotkeys;

const ACTIONS: { field: Field; label: string; hint: string }[] = [
  {
    field: "toggle_replay",
    label: "Start or stop replay",
    hint: "Begins or ends the rolling buffer.",
  },
  {
    field: "save_clip",
    label: "Save clip",
    hint: "Saves the last buffered seconds — needs replay running.",
  },
];

/// Physical key codes that are modifiers on their own; pressing one is not a
/// complete shortcut.
const MODIFIER_CODES = new Set([
  "ControlLeft",
  "ControlRight",
  "ShiftLeft",
  "ShiftRight",
  "AltLeft",
  "AltRight",
  "MetaLeft",
  "MetaRight",
  "OSLeft",
  "OSRight",
  "CapsLock",
  "NumLock",
  "ScrollLock",
]);

const KEY_LABELS: Record<string, string> = {
  Ctrl: "Ctrl",
  Shift: "Shift",
  Alt: "Alt",
  Super: "Win",
  ArrowUp: "↑",
  ArrowDown: "↓",
  ArrowLeft: "←",
  ArrowRight: "→",
  Space: "Space",
  Enter: "Enter",
  Escape: "Esc",
  Backspace: "Backspace",
  Delete: "Del",
  Tab: "Tab",
  Backquote: "`",
  Minus: "-",
  Equal: "=",
  BracketLeft: "[",
  BracketRight: "]",
  Backslash: "\\",
  Semicolon: ";",
  Quote: "'",
  Comma: ",",
  Period: ".",
  Slash: "/",
  NumpadAdd: "Num +",
  NumpadSubtract: "Num −",
  NumpadMultiply: "Num ×",
  NumpadDivide: "Num ÷",
  NumpadDecimal: "Num .",
  NumpadEnter: "Num Enter",
};

/// Builds a Tauri accelerator (e.g. `Ctrl+Shift+KeyR`) from a keydown event.
/// Returns null for modifier-only presses or combinations with no strong
/// modifier, which are too likely to clash with normal typing.
function acceleratorFromEvent(event: KeyboardEvent): string | null {
  if (MODIFIER_CODES.has(event.code)) return null;
  if (!event.ctrlKey && !event.altKey && !event.metaKey) return null;

  const parts: string[] = [];
  if (event.ctrlKey) parts.push("Ctrl");
  if (event.shiftKey) parts.push("Shift");
  if (event.altKey) parts.push("Alt");
  if (event.metaKey) parts.push("Super");
  parts.push(event.code);
  return parts.join("+");
}

function prettyKey(token: string): string {
  if (token in KEY_LABELS) return KEY_LABELS[token];
  if (token.startsWith("Key")) return token.slice(3);
  if (token.startsWith("Digit")) return token.slice(5);
  if (token.startsWith("Numpad")) return `Num ${token.slice(6)}`;
  return token;
}

function formatAccelerator(accelerator: string | null): string {
  if (!accelerator) return "";
  return accelerator.split("+").map(prettyKey).join(" + ");
}

/// Returns a message when the same combination is bound to two actions.
function duplicateError(hotkeys: Hotkeys): string | null {
  const seen = new Map<string, string>();
  for (const { field, label } of ACTIONS) {
    const value = hotkeys[field];
    if (!value) continue;
    const owner = seen.get(value);
    if (owner) return `${formatAccelerator(value)} is bound to both ${owner} and ${label}.`;
    seen.set(value, label);
  }
  return null;
}

export default function HotkeysView() {
  const [hotkeys, setHotkeys] = useState<Hotkeys | null>(null);
  const [initial, setInitial] = useState<Hotkeys | null>(null);
  const [recording, setRecording] = useState<Field | null>(null);
  const [saving, setSaving] = useState(false);
  const [saved, setSaved] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    api
      .getHotkeys()
      .then((loaded) => {
        setHotkeys(loaded);
        setInitial(loaded);
      })
      .catch((caught) => setError(String(caught)));

    return () => {
      // If the view unmounts mid-record, make sure the real bindings return.
      api.setHotkeysSuspended(false).catch(() => {});
    };
  }, []);

  useEffect(() => {
    if (!recording) return;
    const field = recording;

    function onKeyDown(event: KeyboardEvent) {
      event.preventDefault();
      event.stopPropagation();

      if (event.key === "Escape") {
        stopRecording();
        return;
      }

      const clearing =
        (event.key === "Backspace" || event.key === "Delete") &&
        !event.ctrlKey &&
        !event.altKey &&
        !event.metaKey;
      if (clearing) {
        setValue(field, null);
        stopRecording();
        return;
      }

      const accelerator = acceleratorFromEvent(event);
      if (!accelerator) return;

      setValue(field, accelerator);
      stopRecording();
    }

    window.addEventListener("keydown", onKeyDown, true);
    return () => window.removeEventListener("keydown", onKeyDown, true);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [recording]);

  function setValue(field: Field, value: string | null) {
    setHotkeys((current) =>
      current ? ({ ...current, [field]: value } as Hotkeys) : current,
    );
    setSaved(false);
    setError(null);
  }

  async function startRecording(field: Field) {
    if (recording === field) return;
    setError(null);
    try {
      await api.setHotkeysSuspended(true);
      setRecording(field);
    } catch (caught) {
      setError(String(caught));
    }
  }

  function stopRecording() {
    setRecording(null);
    api.setHotkeysSuspended(false).catch(() => {});
  }

  function clear(field: Field) {
    setValue(field, null);
  }

  async function save() {
    if (!hotkeys) return;
    const duplicate = duplicateError(hotkeys);
    if (duplicate) {
      setError(duplicate);
      return;
    }
    setSaving(true);
    try {
      await api.setHotkeys(hotkeys);
      setInitial(hotkeys);
      setSaved(true);
      setError(null);
    } catch (caught) {
      setError(String(caught));
    } finally {
      setSaving(false);
    }
  }

  const dirty =
    hotkeys !== null && JSON.stringify(hotkeys) !== JSON.stringify(initial);

  return (
    <>
      <PageHeader
        title="Hotkeys"
        description="Global shortcuts that work even when Trace is in the background."
      >
        <Button
          variant="primary"
          size="sm"
          loading={saving}
          disabled={!dirty || recording !== null}
          onClick={save}
        >
          Save
        </Button>
      </PageHeader>

      <div className="flex max-w-2xl flex-col gap-4 p-6">
        {error && <Banner tone="error">{error}</Banner>}

        {!hotkeys ? (
          <p className="text-[13px] text-ink-muted">Loading…</p>
        ) : (
          <Panel title="Global shortcuts" bodyClassName="p-0">
            <ul className="divide-y divide-line">
              {ACTIONS.map(({ field, label, hint }) => {
                const value = hotkeys[field];
                const isRecording = recording === field;
                return (
                  <li
                    key={field}
                    className="flex items-center justify-between gap-4 px-4 py-3"
                  >
                    <div className="flex min-w-0 flex-col">
                      <span className="text-sm">{label}</span>
                      <span className="text-[12px] text-ink-faint">{hint}</span>
                    </div>

                    <div className="flex shrink-0 items-center gap-2">
                      <button
                        type="button"
                        onClick={() => startRecording(field)}
                        title={value ? formatAccelerator(value) : "Set a shortcut"}
                        className={cx(
                          "inline-flex h-8 min-w-[9rem] items-center justify-center rounded-[6px] border px-3 text-[12px] transition-colors",
                          isRecording
                            ? "border-accent bg-elevated text-ink"
                            : "border-line bg-elevated text-ink-muted hover:border-line-strong hover:text-ink",
                        )}
                      >
                        {isRecording ? (
                          "Press keys…"
                        ) : value ? (
                          formatAccelerator(value)
                        ) : (
                          <span className="text-ink-faint">Not set</span>
                        )}
                      </button>

                      <button
                        type="button"
                        aria-label={`Clear ${label}`}
                        disabled={!value || isRecording}
                        onClick={() => clear(field)}
                        className="grid size-8 cursor-pointer place-items-center rounded-[6px] border border-transparent text-ink-faint transition-colors hover:bg-elevated hover:text-ink disabled:pointer-events-none disabled:opacity-0"
                      >
                        <X className="size-3.5" />
                      </button>
                    </div>
                  </li>
                );
              })}
            </ul>
          </Panel>
        )}

        <p className="flex items-start gap-2 text-[12px] text-ink-faint">
          <Keyboard className="mt-0.5 size-3.5 shrink-0" />
          <span>
            Click a shortcut, then press the combination. Include Ctrl, Alt, or Win.
            Esc cancels and Backspace clears. Changes take effect when you save.
            {saved && dirty === false && (
              <span className="text-ink-muted"> Saved.</span>
            )}
          </span>
        </p>
      </div>
    </>
  );
}
