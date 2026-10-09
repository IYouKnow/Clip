import { useCallback, useEffect, useState } from "react";

const STORAGE_KEY = "trace.sidebar.collapsed";

function readStored(): boolean {
  try {
    return localStorage.getItem(STORAGE_KEY) === "true";
  } catch {
    // localStorage can be unavailable; fall through to the default.
  }
  return false;
}

/// Remembers whether the sidebar is collapsed across sessions.
export function useSidebarCollapsed() {
  const [collapsed, setCollapsed] = useState(readStored);

  useEffect(() => {
    try {
      localStorage.setItem(STORAGE_KEY, String(collapsed));
    } catch {
      // Persisting is best-effort.
    }
  }, [collapsed]);

  const toggle = useCallback(() => setCollapsed((value) => !value), []);

  return { collapsed, toggle };
}
