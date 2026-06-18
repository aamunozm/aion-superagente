// Configuración de navegación del shell (extraída de AppShell para Atomic Design).
// Una sola fuente de verdad para el sidebar; añadir una sección = una línea aquí.
import type { IconName } from "@/components/atoms";

export type NavItem = { href: string; key: string; icon: IconName };
export type NavGroup = { titleKey: string; items: NavItem[] };

export const NAV_GROUPS: NavGroup[] = [
  {
    titleKey: "group.main",
    items: [
      { href: "/chat", key: "nav.chat", icon: "chat" },
      { href: "/projects", key: "nav.projects", icon: "folder" },
      { href: "/communications", key: "nav.communications", icon: "message" },
    ],
  },
  {
    titleKey: "group.intelligence",
    items: [
      { href: "/tools", key: "nav.tools", icon: "tools" },
      { href: "/memory", key: "nav.memory", icon: "memory" },
      { href: "/mind", key: "nav.mind", icon: "sparkle" },
      { href: "/claude-code", key: "nav.claudeCode", icon: "code" },
    ],
  },
];
