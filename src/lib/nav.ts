import {
  Film,
  Info,
  Keyboard,
  LayoutDashboard,
  Settings,
  type LucideIcon,
} from "lucide-react";

export type View = "dashboard" | "library" | "settings" | "hotkeys" | "about";

export type NavEntry = {
  id: View;
  label: string;
  icon: LucideIcon;
};

export const NAV: NavEntry[] = [
  { id: "dashboard", label: "Dashboard", icon: LayoutDashboard },
  { id: "library", label: "Library", icon: Film },
  { id: "settings", label: "Settings", icon: Settings },
  { id: "hotkeys", label: "Hotkeys", icon: Keyboard },
  { id: "about", label: "About", icon: Info },
];
