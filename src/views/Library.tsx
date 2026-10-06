import { useCallback, useEffect, useState } from "react";
import { api, assetUrl, formatBytes, formatDate, type Clip } from "../lib/api";

export default function Library() {
  const [clips, setClips] = useState<Clip[]>([]);
  const [playing, setPlaying] = useState<Clip | null>(null);
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
    if (!window.confirm(`Delete ${clip.name}?`)) return;
    try {
      await api.deleteClip(clip.path);
      await refresh();
    } catch (caught) {
      setError(String(caught));
    }
  }

  return (
    <div className="stack">
      <section className="card">
        <div className="row space-between">
          <h2>Clips</h2>
          <button className="btn" onClick={refresh} disabled={loading}>
            {loading ? "Refreshing…" : "Refresh"}
          </button>
        </div>
        {error && <div className="banner error">{error}</div>}

        {clips.length === 0 && !loading ? (
          <p className="muted">
            No clips yet. Start replay, then press Save clip on the Replay tab.
          </p>
        ) : (
          <ul className="clip-grid">
            {clips.map((clip) => (
              <li key={clip.path} className="clip-card">
                <button className="clip-thumb" onClick={() => setPlaying(clip)}>
                  <span className="play-glyph">▶</span>
                </button>
                <div className="clip-meta">
                  <div className="clip-name" title={clip.name}>
                    {clip.name}
                  </div>
                  <div className="muted small">
                    {formatBytes(clip.size_bytes)} · {formatDate(clip.modified_ms)}
                  </div>
                </div>
                <div className="clip-actions">
                  <button className="btn small" onClick={() => setPlaying(clip)}>
                    Play
                  </button>
                  <button className="btn small" onClick={() => api.openClip(clip.path)}>
                    Open
                  </button>
                  <button className="btn small" onClick={() => api.revealClip(clip.path)}>
                    Folder
                  </button>
                  <button className="btn small danger" onClick={() => remove(clip)}>
                    Delete
                  </button>
                </div>
              </li>
            ))}
          </ul>
        )}
      </section>

      {playing && (
        <div className="modal-backdrop" onClick={() => setPlaying(null)}>
          <div className="modal" onClick={(event) => event.stopPropagation()}>
            <div className="row space-between">
              <strong>{playing.name}</strong>
              <button className="btn small" onClick={() => setPlaying(null)}>
                Close
              </button>
            </div>
            <video className="player" src={assetUrl(playing.path)} controls autoPlay />
          </div>
        </div>
      )}
    </div>
  );
}
