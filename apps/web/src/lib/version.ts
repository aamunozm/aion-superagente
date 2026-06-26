// Versión visible de la app (la que se muestra en la UI y en «Acerca de AION»).
// ⚠️ MANTENER EN SINCRONÍA con `apps/desktop/src-tauri/tauri.conf.json` (campo `version`,
// = CFBundleShortVersionString del bundle) en cada release, para que lo que ves en pantalla
// coincida con el instalador que tienes.
// CONVENCIÓN: CADA modificación que se despliega sube la versión (número más alto = más reciente).
export const APP_VERSION = "0.3.0";
