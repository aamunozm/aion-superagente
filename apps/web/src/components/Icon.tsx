// Iconos vectoriales minimalistas (trazo, currentColor) — sin emojis 3D.
// Estilo línea coherente, 24×24, stroke 1.7, esquinas redondeadas.

type IconName =
  | "chat" | "folder" | "tools" | "memory" | "settings" | "search" | "send"
  | "mic" | "clock" | "plus" | "user" | "logout" | "sparkle" | "globe" | "code"
  | "calculator" | "eye" | "hand" | "mail" | "calendar" | "leaf" | "bulb"
  | "help" | "warn" | "moon" | "refresh" | "target" | "bot" | "check" | "lock"
  | "download" | "upload" | "brain" | "wave" | "graph" | "cpu" | "shield";

const PATHS: Record<IconName, React.ReactNode> = {
  chat: <path d="M21 15a2 2 0 0 1-2 2H7l-4 4V5a2 2 0 0 1 2-2h14a2 2 0 0 1 2 2z" />,
  folder: <path d="M3 7a2 2 0 0 1 2-2h4l2 2.5h8a2 2 0 0 1 2 2V18a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2z" />,
  tools: <><rect x="3" y="3" width="7" height="7" rx="1.5" /><rect x="14" y="3" width="7" height="7" rx="1.5" /><rect x="3" y="14" width="7" height="7" rx="1.5" /><rect x="14" y="14" width="7" height="7" rx="1.5" /></>,
  brain: <path d="M9 4a3 3 0 0 0-3 3 3 3 0 0 0-1 5.5A3 3 0 0 0 9 18a2.5 2.5 0 0 0 3-2.5V6.5A2.5 2.5 0 0 0 9 4Zm6 0a3 3 0 0 1 3 3 3 3 0 0 1 1 5.5A3 3 0 0 1 15 18a2.5 2.5 0 0 1-3-2.5" />,
  memory: <><circle cx="12" cy="12" r="3" /><circle cx="5" cy="6" r="1.6" /><circle cx="19" cy="7" r="1.6" /><circle cx="6" cy="18" r="1.6" /><path d="M9.6 10.2 6.4 7.2M14.6 11 17.6 8M10 14l-3 3" /></>,
  graph: <><circle cx="6" cy="7" r="2" /><circle cx="18" cy="9" r="2" /><circle cx="9" cy="18" r="2" /><path d="M7.7 8.4 16 9M7.4 8.8 8.6 16M10.8 17l5.6-6.6" /></>,
  settings: <><circle cx="12" cy="12" r="3" /><path d="M19 12a7 7 0 0 0-.1-1l2-1.5-2-3.4-2.3 1a7 7 0 0 0-1.7-1L14.5 2h-4l-.4 2.6a7 7 0 0 0-1.7 1l-2.3-1-2 3.4 2 1.5a7 7 0 0 0 0 2l-2 1.5 2 3.4 2.3-1a7 7 0 0 0 1.7 1l.4 2.6h4l.4-2.6a7 7 0 0 0 1.7-1l2.3 1 2-3.4-2-1.5a7 7 0 0 0 .1-1Z" /></>,
  search: <><circle cx="11" cy="11" r="7" /><path d="m20 20-3.5-3.5" /></>,
  send: <path d="M12 19V5M5 12l7-7 7 7" />,
  mic: <><rect x="9" y="3" width="6" height="11" rx="3" /><path d="M5 11a7 7 0 0 0 14 0M12 18v3" /></>,
  clock: <><circle cx="12" cy="12" r="8.5" /><path d="M12 7.5V12l3 2" /></>,
  plus: <path d="M12 5v14M5 12h14" />,
  user: <><circle cx="12" cy="8" r="4" /><path d="M4 21a8 8 0 0 1 16 0" /></>,
  logout: <path d="M15 4h3a2 2 0 0 1 2 2v12a2 2 0 0 1-2 2h-3M10 17l-5-5 5-5M5 12h11" />,
  sparkle: <path d="M12 3l1.8 5.2L19 10l-5.2 1.8L12 17l-1.8-5.2L5 10l5.2-1.8z" />,
  globe: <><circle cx="12" cy="12" r="8.5" /><path d="M3.5 12h17M12 3.5c2.5 2.5 2.5 14 0 17M12 3.5c-2.5 2.5-2.5 14 0 17" /></>,
  code: <path d="m8 9-3 3 3 3M16 9l3 3-3 3M13 7l-2 10" />,
  calculator: <><rect x="5" y="3" width="14" height="18" rx="2" /><path d="M8 7h8M8 11h.01M12 11h.01M16 11h.01M8 15h.01M12 15h.01M16 15v3M8 18h4" /></>,
  eye: <><path d="M2 12s3.5-7 10-7 10 7 10 7-3.5 7-10 7-10-7-10-7Z" /><circle cx="12" cy="12" r="2.6" /></>,
  hand: <path d="M7 11V6.5a1.5 1.5 0 0 1 3 0V11m0-.5V5a1.5 1.5 0 0 1 3 0v6m0-.5V6.5a1.5 1.5 0 0 1 3 0V13a6 6 0 0 1-6 6h-1a6 6 0 0 1-5-3l-1.5-2.5a1.5 1.5 0 0 1 2.5-1.6L7 13" />,
  mail: <><rect x="3" y="5" width="18" height="14" rx="2" /><path d="m4 7 8 6 8-6" /></>,
  calendar: <><rect x="3" y="5" width="18" height="16" rx="2" /><path d="M3 9h18M8 3v4M16 3v4" /></>,
  leaf: <path d="M5 19c0-8 6-13 14-13 0 8-5 14-13 14-1 0-1-1-1-1Zm2-1 8-8" />,
  bulb: <path d="M9 18h6M10 21h4M8.5 14a5.5 5.5 0 1 1 7 0c-.8.7-1.5 1.4-1.5 2.5h-4c0-1.1-.7-1.8-1.5-2.5Z" />,
  help: <><circle cx="12" cy="12" r="8.5" /><path d="M9.5 9.5a2.5 2.5 0 1 1 3.5 2.3c-.6.3-1 .8-1 1.7M12 17h.01" /></>,
  warn: <path d="M12 4 2.5 20h19L12 4ZM12 10v4M12 17.5h.01" />,
  moon: <path d="M20 14A8 8 0 1 1 10 4a6.5 6.5 0 0 0 10 10Z" />,
  refresh: <path d="M4 9a8 8 0 0 1 13.5-3.5L20 8M20 5v3h-3M20 15a8 8 0 0 1-13.5 3.5L4 16M4 19v-3h3" />,
  target: <><circle cx="12" cy="12" r="8.5" /><circle cx="12" cy="12" r="4.5" /><circle cx="12" cy="12" r="1" /></>,
  bot: <><rect x="4" y="8" width="16" height="11" rx="3" /><path d="M12 8V4M9 13h.01M15 13h.01M2 13v2M22 13v2" /></>,
  check: <path d="m5 12 4.5 4.5L19 7" />,
  lock: <><rect x="5" y="11" width="14" height="9" rx="2" /><path d="M8 11V8a4 4 0 0 1 8 0v3" /></>,
  download: <path d="M12 4v11M7 11l5 5 5-5M5 20h14" />,
  upload: <path d="M12 20V9M7 13l5-5 5 5M5 4h14" />,
  wave: <path d="M2 12h3l2-6 4 14 3-9 2 4h6" />,
  cpu: <><rect x="6" y="6" width="12" height="12" rx="2" /><rect x="9.5" y="9.5" width="5" height="5" rx="1" /><path d="M9 3v3M15 3v3M9 18v3M15 18v3M3 9h3M3 15h3M18 9h3M18 15h3" /></>,
  shield: <path d="M12 3 5 6v5c0 4.5 3 8 7 10 4-2 7-5.5 7-10V6Z" />,
};

export default function Icon({
  name,
  size = 20,
  className,
  strokeWidth = 1.7,
}: {
  name: IconName;
  size?: number;
  className?: string;
  strokeWidth?: number;
}) {
  return (
    <svg
      width={size}
      height={size}
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth={strokeWidth}
      strokeLinecap="round"
      strokeLinejoin="round"
      className={className}
      aria-hidden="true"
    >
      {PATHS[name]}
    </svg>
  );
}
