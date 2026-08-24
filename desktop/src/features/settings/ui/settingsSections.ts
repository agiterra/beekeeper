import {
  Archive,
  BellRing,
  Bot,
  Cpu,
  Download,
  FlaskConical,
  Keyboard,
  LayoutTemplate,
  MessagesSquare,
  MonitorCog,
  ShieldAlert,
  Smartphone,
  Smile,
  SquareTerminal,
  Terminal,
  Ticket,
  UserRound,
  Volume2,
  type LucideIcon,
} from "lucide-react";

/**
 * The settings-section registry: the union of section ids, the navigation
 * descriptors (label + icon + optional feature gate), and the id guard.
 * Split from `SettingsPanels.tsx` purely for size — the panel bodies stay
 * there; this file owns only what a section *is*.
 */
export type SettingsSection =
  | "profile"
  | "notifications"
  | "voice"
  | "experimental"
  | "agents"
  | "channel-templates"
  | "sessions"
  | "compute"
  | "appearance"
  | "shortcuts"
  | "hosted-communities"
  | "community-members"
  | "moderation"
  | "custom-emoji"
  | "builtin-shell"
  | "local-archive"
  | "mobile"
  | "updates";

export const DEFAULT_SETTINGS_SECTION: SettingsSection = "profile";

const SETTINGS_SECTION_VALUES: readonly SettingsSection[] = [
  "profile",
  "notifications",
  "voice",
  "sessions",
  "experimental",
  "agents",
  "channel-templates",
  "compute",
  "appearance",
  "shortcuts",
  "hosted-communities",
  "community-members",
  "moderation",
  "custom-emoji",
  "builtin-shell",
  "local-archive",
  "mobile",
  "updates",
];

export function isSettingsSection(value: unknown): value is SettingsSection {
  return (
    typeof value === "string" &&
    (SETTINGS_SECTION_VALUES as readonly string[]).includes(value)
  );
}

export type SettingsSectionDescriptor = {
  value: SettingsSection;
  label: string;
  icon: LucideIcon;
  /** If set, this section is only visible when the feature is enabled */
  featureGate?: string;
};

export const settingsSections: SettingsSectionDescriptor[] = [
  {
    value: "appearance",
    label: "Appearance",
    icon: MonitorCog,
  },
  {
    value: "profile",
    label: "Profile",
    icon: UserRound,
  },
  {
    value: "notifications",
    label: "Notifications",
    icon: BellRing,
  },
  {
    value: "voice",
    label: "Voice",
    icon: Volume2,
  },
  {
    value: "experimental",
    label: "Experiments",
    icon: FlaskConical,
  },
  {
    value: "agents",
    label: "Agents",
    icon: Bot,
    featureGate: "managed-agents",
  },
  {
    value: "channel-templates",
    label: "Channel templates",
    icon: LayoutTemplate,
    featureGate: "channel-templates",
  },
  {
    value: "sessions",
    label: "Sessions",
    icon: SquareTerminal,
  },
  {
    value: "compute",
    label: "Compute",
    icon: Cpu,
  },
  {
    value: "shortcuts",
    label: "Shortcuts",
    icon: Keyboard,
  },
  {
    value: "hosted-communities",
    label: "Hosted communities",
    icon: MessagesSquare,
  },
  {
    value: "community-members",
    label: "Invites",
    icon: Ticket,
  },
  {
    value: "moderation",
    label: "Moderation",
    icon: ShieldAlert,
  },
  {
    value: "custom-emoji",
    label: "Custom emoji",
    icon: Smile,
    featureGate: "custom-emoji",
  },
  {
    value: "builtin-shell",
    label: "Terminals",
    icon: Terminal,
    featureGate: "builtin-shell",
  },
  {
    value: "local-archive",
    label: "Local archive",
    icon: Archive,
  },
  {
    value: "mobile",
    label: "Mobile",
    icon: Smartphone,
  },
  {
    value: "updates",
    label: "Updates",
    icon: Download,
  },
];
