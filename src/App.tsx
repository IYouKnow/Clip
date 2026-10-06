import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import "./App.css";

type EngineStatus = {
  replaying: boolean;
  encoder: string | null;
  buffer_seconds: number;
};

function App() {
  const [status, setStatus] = useState<EngineStatus | null>(null);

  useEffect(() => {
    invoke<EngineStatus>("get_status").then(setStatus).catch(console.error);
  }, []);

  return (
    <main className="container">
      <h1>Clipper23</h1>
      <p>Clip anything on your screen. Engine not wired up yet.</p>
      <dl>
        <dt>Replaying</dt>
        <dd>{status ? String(status.replaying) : "…"}</dd>
        <dt>Encoder</dt>
        <dd>{status?.encoder ?? "none"}</dd>
        <dt>Buffer</dt>
        <dd>{status ? `${status.buffer_seconds}s` : "…"}</dd>
      </dl>
    </main>
  );
}

export default App;
