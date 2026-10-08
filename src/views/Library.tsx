import { Film, FolderOpen, Play, RefreshCw, Trash2 } from "lucide-react";
import { useCallback, useEffect, useState } from "react";
import { Banner } from "../components/ui/Banner";
import { Button } from "../components/ui/Button";
import { ConfirmDialog } from "../components/ui/ConfirmDialog";
import { EmptyState } from "../components/ui/EmptyState";
import { Modal } from "../components/ui/Modal";
import { PageHeader } from "../components/ui/PageHeader";
import { Panel } from "../components/ui/Panel";
import { api, assetUrl, formatBytes, formatDate, type Clip } from "../lib/api";

export default function Library() {
  const [clips, setClips] = useState<Clip[]>([]);
  const [playing, setPlaying] = useState<Clip | null>(null);
  const [pendingDelete, setPendingDelete] = useState<Clip | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(true);

  const refresh = useCallback(async () => {
    setLoading(true);
    try {
      setClips(await api.listClips());
      setError(null);
    } catch (caught) {
      setError(String(caught));
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    refresh();
  }, [refresh]);

  async function remove(clip: Clip) {
    setPendingDelete(null);
    try {
      await api.deleteClip(clip.path);
      await refresh();
    } catch (caught) {
      setError(String(caught));
    }
  }

  return (
    <>
      <PageHeader title="Library" description="Clips saved under your Videos folder.">
        <Button
          disabled={loading}
          icon={<RefreshCw className="size-4" />}
          onClick={refresh}
        >
          {loading ? "Refreshing…" : "Refresh"}
        </Button>
      </PageHeader>

      <div className="flex flex-col gap-4 p-6">
        {error && <Banner tone="error">{error}</Banner>}

        {clips.length === 0 ? (
          <Panel>
            {loading ? (
              <p className="text-[13px] text-ink-muted">Loading clips…</p>
            ) : (
              <EmptyState
                icon={Film}
                title="No clips yet"
                description="Start replay on Home, then press Save clip. Saved clips show up here."
              />
            )}
          </Panel>
        ) : (
          <ul className="grid grid-cols-[repeat(auto-fill,minmax(220px,1fr))] gap-3.5">
            {clips.map((clip) => (
              <li
                key={clip.path}
                className="flex flex-col overflow-hidden rounded-[var(--radius-panel)] border border-line bg-surface"
              >
                <button
                  type="button"
                  aria-label={`Play ${clip.name}`}
                  onClick={() => setPlaying(clip)}
                  className="grid h-28 cursor-pointer place-items-center border-b border-line bg-elevated text-ink-muted transition-colors duration-150 hover:text-ink"
                >
                  <Play className="size-6" />
                </button>
                <div className="px-3 pt-2.5">
                  <div className="truncate text-sm font-medium" title={clip.name}>
                    {clip.name}
                  </div>
                  <div className="mt-0.5 text-[12px] text-ink-muted">
                    {formatBytes(clip.size_bytes)} · {formatDate(clip.modified_ms)}
                  </div>
                </div>
                <div className="flex flex-wrap gap-1.5 p-3">
                  <Button size="sm" onClick={() => setPlaying(clip)}>
                    Play
                  </Button>
                  <Button size="sm" onClick={() => api.openClip(clip.path)}>
                    Open
                  </Button>
                  <Button
                    size="sm"
                    icon={<FolderOpen className="size-3.5" />}
                    onClick={() => api.revealClip(clip.path)}
                  >
                    Folder
                  </Button>
                  <Button
                    size="sm"
                    variant="danger"
                    icon={<Trash2 className="size-3.5" />}
                    onClick={() => setPendingDelete(clip)}
                  >
                    Delete
                  </Button>
                </div>
              </li>
            ))}
          </ul>
        )}
      </div>

      {playing && (
        <Modal title={playing.name} onClose={() => setPlaying(null)}>
          <div className="mb-3 flex items-center justify-between gap-3">
            <strong className="truncate text-sm" title={playing.name}>
              {playing.name}
            </strong>
            <Button size="sm" onClick={() => setPlaying(null)}>
              Close
            </Button>
          </div>
          <video
            className="max-h-[60vh] w-full rounded-[var(--radius-control)] bg-black"
            src={assetUrl(playing.path)}
            controls
            autoPlay
          />
        </Modal>
      )}

      {pendingDelete && (
        <ConfirmDialog
          title="Delete clip"
          message={`Delete ${pendingDelete.name}? This removes the file from disk and cannot be undone.`}
          confirmLabel="Delete clip"
          danger
          onCancel={() => setPendingDelete(null)}
          onConfirm={() => remove(pendingDelete)}
        />
      )}
    </>
  );
}
