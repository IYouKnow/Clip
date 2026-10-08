import {
  Film,
  Home,
  Keyboard,
  Settings,
  type LucideIcon,
} from "lucide-react";

export type View = "home" | "library" | "settings" | "hotkeys";

export type NavEntry = {
  id: View;
  label: string;
  icon: LucideIcon;
};

export const NAV: NavEntry[] = [
  { id: "home", label: "Home", icon: Home },
  { id: "library", label: "Library", icon: Film },
  { id: "settings", label: "Settings", icon: Settings },
  { id: "hotkeys", label: "Hotkeys", icon: Keyboard },
];
