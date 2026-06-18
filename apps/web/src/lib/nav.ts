// Configuración de navegación del shell (extraída de AppShell para Atomic Design).
// Una sola fuente de verdad para el sidebar; añadir una sección = una línea aquí.
import type { IconName } from "@/components/atoms";

// `badgeKey` (opcional): marca el ítem para mostrar un contador dinámico (p. ej. avisos
// sin leer de la Bandeja). El AppShell resuelve el número y pinta la píldora.
export type NavItem = { href: string; key: string; icon: IconName; badgeKey?: "inbox" };
export type NavGroup = { titleKey: string; items: NavItem[] };

export const NAV_GROUPS: NavGroup[] = [
  {
    titleKey: "group.main",
    items: [
      { href: "/chat", key: "nav.chat", icon: "chat" },
      { href: "/inbox", key: "nav.inbox", icon: "bell", badgeKey: "inbox" },
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
