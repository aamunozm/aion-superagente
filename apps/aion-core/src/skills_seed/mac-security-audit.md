---
name: mac-security-audit
description: Revisa la postura de seguridad del Mac (firewall, FileVault, SIP, Gatekeeper, login items)
when_to_use: "revisa mi seguridad", "estoy protegido", auditoría de seguridad, firewall, filevault
category: sistema
tools: run_command
---
Objetivo: informe de seguridad de solo lectura, sin cambiar ajustes.

Procedimiento:
1. Recoge el estado (un comando, todo read-only):
   `echo '---FIREWALL---'; /usr/libexec/ApplicationFirewall/socketfilterfw --getglobalstate 2>/dev/null; echo '---FILEVAULT---'; fdesetup status 2>/dev/null; echo '---SIP---'; csrutil status 2>/dev/null; echo '---GATEKEEPER---'; spctl --status 2>/dev/null; echo '---LOGIN ITEMS---'; osascript -e 'tell application "System Events" to get the name of every login item' 2>/dev/null`
2. Por cada control: ✅ activado / 🔴 desactivado, y qué implica.
3. Lista los login items (apps que arrancan solas) y señala los que no reconozcas.
4. Cierra con prioridades: qué activar primero y cómo (indicaciones, NO lo cambies tú: son ajustes de seguridad).

Reglas: solo LECTURA. Nunca desactives protecciones ni uses sudo. Cambios de seguridad los hace Ariel.
