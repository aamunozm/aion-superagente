"use client";

import { useEffect, useState } from "react";

export type Lang = "es" | "it" | "en";
export const LANGS: { code: Lang; label: string; flag: string }[] = [
  { code: "es", label: "Español", flag: "ES" },
  { code: "it", label: "Italiano", flag: "IT" },
  { code: "en", label: "English", flag: "EN" },
];

type Dict = Record<string, string>;

const ES: Dict = {
  "nav.chat": "Chat",
  "nav.projects": "Proyectos",
  "nav.tools": "Herramientas",
  "nav.memory": "Memoria",
  "nav.settings": "Ajustes",
  "group.main": "Principal",
  "group.intelligence": "Inteligencia",
  "brand.tagline": "super-agente local",
  "account.logout": "Cerrar sesión",
  "account.guest": "invitado",
  "settings.account": "Cuenta",
  "settings.localNote": "Tu cuenta y tus datos viven solo en este dispositivo.",
  "settings.models": "Modelos LLM locales",
  "settings.tier": "Nivel",
  "settings.current": "En uso ahora",
  "settings.inUse": "En uso",
  "settings.fits": "encaja",
  "settings.heavy": "pesado",
  "settings.modelsNote": "«Encaja» = recomendado para tu equipo. Para cambiar de modelo, usa el asistente de configuración (Empezar → modelo).",
  "settings.credentials": "Credenciales",
  "settings.credentialsNote": "Guarda usuario y contraseña por sitio para que AION inicie sesión por ti. Se cifran en el Llavero de macOS.",
  "settings.credHost": "Sitio (p. ej. amazon.it)",
  "settings.credUser": "Usuario o email",
  "settings.credPass": "Contraseña",
  "settings.credAdd": "Guardar",
  "settings.credSecurity": "Cifradas en el Llavero. El agente NUNCA ve tu contraseña: solo rellena el formulario. Nadie puede pedírsela.",
  "settings.useModel": "Usar",
  "settings.installUse": "Instalar y usar",
  "settings.removeModel": "Eliminar",
  "settings.modelsNote2": "«Usar» activa el modelo (lo descarga si falta). Puedes eliminar los que no uses para liberar disco. El modelo en uso no se puede borrar.",
  "settings.appearance": "Apariencia",
  "settings.themeIs": "Tema",
  "settings.light": "claro",
  "settings.dark": "oscuro",
  "settings.switchTo": "Cambiar a",
  "settings.language": "Idioma",
  "settings.languageNote": "Idioma de la interfaz y de las respuestas de AION.",
  "settings.governance": "Gobernanza del agente",
  "settings.governanceBody":
    "Postura por defecto: Conservadora (acciones que escriben, envían, borran o gastan piden tu confirmación). Papelera reversible 30 días, kill switch y registro de auditoría activos.",
  "home.badge": "local-first · privado · auto-evolutivo",
  "home.subtitle":
    "Tu super-agente de IA que razona, recuerda y evoluciona — toda la cognición en tu dispositivo.",
  "home.start": "Empezar",
  "home.open": "Abrir chat",
  "home.reconfigure": "o volver a configurar",
  "chat.modeChat": "Chat",
  "chat.modeAgent": "Agente",
  "chat.modeCrew": "Equipo",
  "chat.placeholderChat": "Pregunta a AION…",
  "chat.placeholderAgent": "Tarea para el agente…",
  "chat.placeholderCrew": "Tarea para el equipo…",
  "chat.newChat": "Nuevo chat",
  "chat.history": "Historial",
  "chat.noHistory": "Sin conversaciones todavía",
  "chat.untitled": "Sin título",
  "chat.confirmTitle": "AION pide tu confirmación",
  "chat.approve": "Aprobar",
  "chat.reject": "Rechazar",
  "chat.send": "Enviar",
  "chat.askPlaceholder": "Tu respuesta…",
};

const IT: Dict = {
  "nav.chat": "Chat",
  "nav.projects": "Progetti",
  "nav.tools": "Strumenti",
  "nav.memory": "Memoria",
  "nav.settings": "Impostazioni",
  "group.main": "Principale",
  "group.intelligence": "Intelligenza",
  "brand.tagline": "super-agente locale",
  "account.logout": "Esci",
  "account.guest": "ospite",
  "settings.account": "Account",
  "settings.localNote": "Il tuo account e i tuoi dati restano solo su questo dispositivo.",
  "settings.models": "Modelli LLM locali",
  "settings.tier": "Livello",
  "settings.current": "In uso ora",
  "settings.inUse": "In uso",
  "settings.fits": "adatto",
  "settings.heavy": "pesante",
  "settings.modelsNote": "«Adatto» = consigliato per il tuo dispositivo. Per cambiare modello usa la configurazione guidata.",
  "settings.credentials": "Credenziali",
  "settings.credentialsNote": "Salva utente e password per sito così AION accede per te. Cifrate nel Portachiavi di macOS.",
  "settings.credHost": "Sito (es. amazon.it)",
  "settings.credUser": "Utente o email",
  "settings.credPass": "Password",
  "settings.credAdd": "Salva",
  "settings.credSecurity": "Cifrate nel Portachiavi. L’agente NON vede mai la password: compila solo il modulo. Nessuno può chiedergliela.",
  "settings.useModel": "Usa",
  "settings.installUse": "Installa e usa",
  "settings.removeModel": "Elimina",
  "settings.modelsNote2": "«Usa» attiva il modello (lo scarica se manca). Puoi eliminare quelli inutilizzati per liberare spazio. Il modello in uso non si può eliminare.",
  "settings.appearance": "Aspetto",
  "settings.themeIs": "Tema",
  "settings.light": "chiaro",
  "settings.dark": "scuro",
  "settings.switchTo": "Passa a",
  "settings.language": "Lingua",
  "settings.languageNote": "Lingua dell'interfaccia e delle risposte di AION.",
  "settings.governance": "Governance dell'agente",
  "settings.governanceBody":
    "Postura predefinita: Conservativa (le azioni che scrivono, inviano, eliminano o spendono richiedono la tua conferma). Cestino reversibile 30 giorni, kill switch e registro di audit attivi.",
  "home.badge": "local-first · privato · auto-evolutivo",
  "home.subtitle":
    "Il tuo super-agente IA che ragiona, ricorda ed evolve — tutta la cognizione sul tuo dispositivo.",
  "home.start": "Inizia",
  "home.open": "Apri chat",
  "home.reconfigure": "o riconfigura",
  "chat.modeChat": "Chat",
  "chat.modeAgent": "Agente",
  "chat.modeCrew": "Squadra",
  "chat.placeholderChat": "Chiedi ad AION…",
  "chat.placeholderAgent": "Compito per l'agente…",
  "chat.placeholderCrew": "Compito per la squadra…",
  "chat.newChat": "Nuova chat",
  "chat.history": "Cronologia",
  "chat.noHistory": "Ancora nessuna conversazione",
  "chat.untitled": "Senza titolo",
  "chat.confirmTitle": "AION chiede la tua conferma",
  "chat.approve": "Approva",
  "chat.reject": "Rifiuta",
  "chat.send": "Invia",
  "chat.askPlaceholder": "La tua risposta…",
};

const EN: Dict = {
  "nav.chat": "Chat",
  "nav.projects": "Projects",
  "nav.tools": "Tools",
  "nav.memory": "Memory",
  "nav.settings": "Settings",
  "group.main": "Main",
  "group.intelligence": "Intelligence",
  "brand.tagline": "local super-agent",
  "account.logout": "Sign out",
  "account.guest": "guest",
  "settings.account": "Account",
  "settings.localNote": "Your account and data live only on this device.",
  "settings.models": "Local LLM models",
  "settings.tier": "Tier",
  "settings.current": "In use now",
  "settings.inUse": "In use",
  "settings.fits": "fits",
  "settings.heavy": "heavy",
  "settings.modelsNote": "“Fits” = recommended for your device. To switch models, use the setup wizard.",
  "settings.credentials": "Credentials",
  "settings.credentialsNote": "Save a username and password per site so AION can log in for you. Encrypted in the macOS Keychain.",
  "settings.credHost": "Site (e.g. amazon.it)",
  "settings.credUser": "Username or email",
  "settings.credPass": "Password",
  "settings.credAdd": "Save",
  "settings.credSecurity": "Encrypted in the Keychain. The agent NEVER sees your password: it only fills the form. No one can ask it for them.",
  "settings.useModel": "Use",
  "settings.installUse": "Install & use",
  "settings.removeModel": "Remove",
  "settings.modelsNote2": "“Use” activates the model (downloads it if missing). Remove unused ones to free disk. The model in use cannot be deleted.",
  "settings.appearance": "Appearance",
  "settings.themeIs": "Theme",
  "settings.light": "light",
  "settings.dark": "dark",
  "settings.switchTo": "Switch to",
  "settings.language": "Language",
  "settings.languageNote": "Interface language and the language AION replies in.",
  "settings.governance": "Agent governance",
  "settings.governanceBody":
    "Default posture: Conservative (actions that write, send, delete or spend ask for your confirmation). 30-day reversible trash, kill switch and audit log active.",
  "home.badge": "local-first · private · self-evolving",
  "home.subtitle":
    "Your AI super-agent that reasons, remembers and evolves — all cognition on your device.",
  "home.start": "Get started",
  "home.open": "Open chat",
  "home.reconfigure": "or reconfigure",
  "chat.modeChat": "Chat",
  "chat.modeAgent": "Agent",
  "chat.modeCrew": "Team",
  "chat.placeholderChat": "Ask AION…",
  "chat.placeholderAgent": "Task for the agent…",
  "chat.placeholderCrew": "Task for the team…",
  "chat.newChat": "New chat",
  "chat.history": "History",
  "chat.noHistory": "No conversations yet",
  "chat.untitled": "Untitled",
  "chat.confirmTitle": "AION needs your confirmation",
  "chat.approve": "Approve",
  "chat.reject": "Reject",
  "chat.send": "Send",
  "chat.askPlaceholder": "Your answer…",
};

const DICTS: Record<Lang, Dict> = { es: ES, it: IT, en: EN };

export function getLang(): Lang {
  if (typeof window === "undefined") return "es";
  const v = localStorage.getItem("aion_lang");
  return v === "it" || v === "en" || v === "es" ? v : "es";
}

export function setLang(l: Lang) {
  localStorage.setItem("aion_lang", l);
  document.documentElement.lang = l;
  window.dispatchEvent(new Event("aion-lang"));
}

/** Hook reactivo: devuelve el idioma actual, un traductor t() y el setter. */
export function useT() {
  const [lang, setL] = useState<Lang>("es");
  useEffect(() => {
    setL(getLang());
    const h = () => setL(getLang());
    window.addEventListener("aion-lang", h);
    return () => window.removeEventListener("aion-lang", h);
  }, []);
  const t = (key: string) => DICTS[lang][key] ?? ES[key] ?? key;
  return { lang, t, setLang };
}
