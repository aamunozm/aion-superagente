"use client";

import Link from "next/link";
import { usePathname, useRouter } from "next/navigation";
import { useEffect, useState } from "react";
import Icon from "./Icon";

type NavItem = { href: string; label: string; icon: "chat" | "folder" | "tools" | "memory" };
type NavGroup = { title: string; items: NavItem[] };

const GROUPS: NavGroup[] = [
  {
    title: "Principal",
    items: [
      { href: "/chat", label: "Chat", icon: "chat" },
      { href: "/projects", label: "Proyectos", icon: "folder" },
    ],
  },
  {
    title: "Inteligencia",
    items: [
      { href: "/tools", label: "Herramientas", icon: "tools" },
      { href: "/memory", label: "Memoria", icon: "memory" },
    ],
  },
];

/**
 * Shell interno con SIDEBAR (estilo CEO·Intelligence): navegación a la izquierda,
 * marca arriba, cuenta abajo, y el contenido de cada sección a la derecha.
 */
export default function AppShell({
  title,
  children,
}: {
  title: string;
  children: React.ReactNode;
}) {
  const pathname = usePathname();
  const router = useRouter();
  const [email, setEmail] = useState<string | null>(null);
  const [collapsed, setCollapsed] = useState(false);

  useEffect(() => {
    setEmail(localStorage.getItem("aion_email"));
    // Guard: si no hay sesión, al login.
    if (!localStorage.getItem("aion_token")) router.replace("/login");
  }, [router]);

  function logout() {
    localStorage.removeItem("aion_token");
    localStorage.removeItem("aion_email");
    router.replace("/login");
  }

  return (
    <div className="min-h-screen flex" style={{ background: "var(--bg)" }}>
      {/* ── SIDEBAR ─────────────────────────────────────────── */}
      <aside
        className="flex flex-col shrink-0 transition-all"
        style={{
          width: collapsed ? 68 : 248,
          borderRight: "1px solid var(--border)",
          background: "var(--surface-1)",
        }}
      >
        {/* Marca */}
        <div className="flex items-center gap-2 px-4 h-16 shrink-0">
          <span
            className="w-8 h-8 rounded-lg flex items-center justify-center font-bold shrink-0"
            style={{ background: "var(--accent)", color: "#04201f" }}
          >
            A
          </span>
          {!collapsed && (
            <div className="leading-tight">
              <div className="font-display font-bold text-sm">AION</div>
              <div className="text-[10px]" style={{ color: "var(--text-3)" }}>
                super-agente local
              </div>
            </div>
          )}
          <button
            onClick={() => setCollapsed((c) => !c)}
            className="ml-auto text-xs opacity-50 hover:opacity-100"
            title="Plegar"
          >
            {collapsed ? "›" : "‹"}
          </button>
        </div>

        {/* Navegación agrupada por secciones */}
        <nav className="flex flex-col gap-4 px-3 mt-3">
          {GROUPS.map((group) => (
            <div key={group.title} className="flex flex-col gap-1">
              {!collapsed && (
                <span
                  className="px-3 mb-1 text-[10px] font-semibold uppercase tracking-[0.12em]"
                  style={{ color: "var(--text-3)" }}
                >
                  {group.title}
                </span>
              )}
              {group.items.map((item) => {
                const active = pathname === item.href;
                return (
                  <Link
                    key={item.href}
                    href={item.href}
                    className="flex items-center gap-3 px-3 py-2 rounded-lg text-sm transition-all"
                    style={{
                      background: active ? "var(--accent-subtle)" : "transparent",
                      color: active ? "var(--gold-deep)" : "var(--text-2)",
                      fontWeight: active ? 600 : 500,
                    }}
                    title={item.label}
                  >
                    <Icon name={item.icon} size={18} className="shrink-0" />
                    {!collapsed && <span>{item.label}</span>}
                  </Link>
                );
              })}
            </div>
          ))}
        </nav>

        <div className="mt-auto" />

        {/* Cuenta */}
        <div className="px-3 pb-4">
          <Link
            href="/settings"
            className="flex items-center gap-3 px-3 py-2 rounded-lg text-sm"
            style={{ color: "var(--text-2)" }}
          >
            <Icon name="settings" size={18} />
            {!collapsed && <span>Ajustes</span>}
          </Link>
          <div
            className="mt-2 flex items-center gap-2 px-3 py-2 rounded-lg"
            style={{ background: "var(--surface-2)" }}
          >
            <span
              className="w-7 h-7 rounded-full flex items-center justify-center text-xs font-semibold shrink-0"
              style={{ background: "var(--ink)", color: "#fff" }}
            >
              {(email ?? "?").charAt(0).toUpperCase()}
            </span>
            {!collapsed && (
              <div className="min-w-0 flex-1">
                <div className="text-xs truncate" style={{ color: "var(--text-2)" }}>
                  {email ?? "invitado"}
                </div>
                <button
                  onClick={logout}
                  className="text-[11px]"
                  style={{ color: "var(--text-3)" }}
                >
                  Cerrar sesión
                </button>
              </div>
            )}
          </div>
        </div>
      </aside>

      {/* ── CONTENIDO ───────────────────────────────────────── */}
      <div className="flex-1 flex flex-col min-w-0">
        <header
          className="h-16 shrink-0 flex items-center px-8"
          style={{ borderBottom: "1px solid var(--border)" }}
        >
          <h1 className="font-display font-semibold text-[18px] tracking-tight">{title}</h1>
        </header>
        <main className="flex-1 min-h-0 overflow-y-auto">{children}</main>
      </div>
    </div>
  );
}
