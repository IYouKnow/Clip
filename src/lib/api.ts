import { convertFileSrc, invoke } from "@tauri-apps/api/core";
import { getVersion } from "@tauri-apps/api/app";

export type Status = {
  replaying: boolean;
  encoder: string | null;
  pipeline: string | null;
  frames: number;
  packets: number;
  dropped: number;
  idle: number;
  audio: string | null;
  buffer_seconds: number;
  fps: number;
  bitrate: number;
  clips_dir: string;
  available_encoders: string[];
};

export type Clip = {
  path: string;
  name: string;
  size_bytes: number;
  modified_ms: number;
};

export type AudioSource = "off" | "system" | "microphone" | "both";

export type DeviceInfo = {
  id: string;
  track: string;
  name: string;
  sample_rate: number;
  channels: number;
  bits_per_sample: number;
  is_float: boolean;
};

export type AudioDevices = {
  system: DeviceInfo[];
  microphone: DeviceInfo[];
};

export type Settings = {
  buffer_seconds: number;
  fps: number;
  bitrate: number;
  encoder: string | null;
  audio_source: AudioSource;
  system_device: string | null;
  microphone_device: string | null;
};

export type Hotkeys = {
  toggle_replay: string | null;
  save_clip: string | null;
};

export const api = {
  getStatus: () => invoke<Status>("get_status"),
  startReplay: () => invoke<Status>("start_replay"),
  stopReplay: () => invoke<Status>("stop_replay"),
  saveClip: () => invoke<Clip>("save_clip"),

  listClips: () => invoke<Clip[]>("list_clips"),
  deleteClip: (path: string) => invoke<void>("delete_clip", { path }),
  openClip: (path: string) => invoke<void>("open_clip", { path }),
  revealClip: (path: string) => invoke<void>("reveal_clip", { path }),

  getSettings: () => invoke<Settings>("get_settings"),
  setSettings: (settings: Settings) => invoke<void>("set_settings", { settings }),
  listAudioDevices: () => invoke<AudioDevices>("list_audio_devices"),

  getHotkeys: () => invoke<Hotkeys>("get_hotkeys"),
  setHotkeys: (hotkeys: Hotkeys) => invoke<void>("set_hotkeys", { hotkeys }),
  setHotkeysSuspended: (suspended: boolean) =>
    invoke<void>("set_hotkeys_suspended", { suspended }),

  getVersion: () => getVersion(),
};

/// Turns an absolute path into a URL the webview can load.
export const assetUrl = (path: string) => convertFileSrc(path);

export function formatBytes(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  if (bytes < 1024 * 1024 * 1024) return `${(bytes / 1024 / 1024).toFixed(1)} MB`;
  return `${(bytes / 1024 / 1024 / 1024).toFixed(2)} GB`;
}

export function formatDate(ms: number): string {
  if (!ms) return "";
  return new Date(ms).toLocaleString();
}
