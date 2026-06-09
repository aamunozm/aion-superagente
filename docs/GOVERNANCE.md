# AION — Gobernanza del control del computador

AION puede operar tu Mac (archivos, apps, documentos, email, navegador, sistema),
pero **toda acción pasa por un motor de políticas determinista** antes de ejecutarse.
La seguridad es **código**, no depende del modelo (que es sin censura) ni puede
saltarse con un *prompt* ni con un email malicioso (*prompt injection*).

> El modelo **propone**; el Governor **dispone**. Implementado en `crates/aion-computer`.

## Configuración actual (elegida por Ariel)

| Ajuste | Valor |
|---|---|
| **Postura** | **Conservadora** — autónomo solo para leer/ver/investigar; todo lo que escribe, envía, borra, instala o gasta **pide confirmación** |
| **Alcance** | Control total: Documentos/Office, Email, Archivos/Finder, Navegador + apps, sistema (pantalla, teclado/ratón, shell) |
| **Email** | Leer + **enviar con confirmación** (te muestra el correo y das OK) |
| **Borrado** | **Papelera AION reversible** — nunca borra de verdad; retención 30 días |

Editable en `~/Library/Application Support/AION/policy.json` o desde la app.

## Niveles de decisión

🟢 **Autónomo** · 🟡 **Pide confirmación (HITL)** · 🔴 **Prohibido (línea roja)**

## La lista de reglas

### 1. Dinero y compras
- 🔴 Banca, transferencias, inversiones, trading → **nunca** (lista roja, en cualquier postura).
- 🟡 Comprar, pagar, suscribir, introducir tarjeta → **siempre** confirmación; jamás autónomo. Deja el carrito listo, el clic final es tuyo.

### 2. Borrado y datos personales
- 🟢 Borrar = mover a la **papelera AION** (recuperable 30 días). Nunca borrado real autónomo.
- 🟡 Carpetas protegidas (`Documents`, `Desktop`, `Pictures`, `Movies`, `Library/Keychains`, `.ssh`) → solo lectura salvo confirmación.
- 🟢 Snapshot/copia antes de cambios sobre archivos existentes.

### 3. Comunicaciones en tu nombre
- 🟡 Email/mensajes/redes → redacta siempre; **enviar pide tu OK** con previsualización.
- 🔴 Suplantarte en algo legal/contractual o aceptar términos en tu nombre.

### 4. Privacidad y datos sensibles
- 🔴 Leer/exportar llavero, contraseñas, banca, salud, credenciales → denegado salvo permiso explícito.
- 🟢 Procesamiento **local** (tu Gemma local); nada sale del PC sin confirmación.

### 5. Sistema y seguridad
- 🔴 Desactivar firewall, FileVault, Gatekeeper; `sudo`/escalada de privilegios; `rm -rf /`, formatear.
- 🟡 Instalar/desinstalar software, cambiar ajustes del sistema.

### 6. Red e internet
- 🟢 Navegar, leer e investigar (navegador propio).
- 🟡 Descargar ejecutables / abrir adjuntos desconocidos.
- 🛡️ Lo que lee en web/email es **dato, no orden**: una página no puede cambiar tus reglas.

### 7. Límites de autonomía
- 🛑 **Kill switch** global: pausa total inmediata (deniega todo).
- 🟡 Acciones irreversibles o de alto impacto → confirmación + previsualización.

### 8. Trazabilidad
- 🟢 **Audit log** de cada acción (qué, cuándo, decisión, ejecución, resultado) en `audit.jsonl`.
- 🟢 **Dry-run**: te muestra qué haría antes de hacerlo.

## Cómo se aplica (modelo de ejecución)

```
LLM (gemma4-reason, sin censura)
        │  propone Action { verb, category, reversibility, target, summary }
        ▼
   Governor.authorize(action)  ──► consulta Policy (determinista)
        │
        ├─ Allow   → ejecutar (aplicando salvaguardas: snapshot/papelera/preview)
        ├─ Confirm → PEDIR confirmación al usuario; ejecutar solo si dice sí
        └─ Deny    → NO ejecutar (línea roja)
        │
        ▼  todo queda en el Audit log
```

## Estado de implementación

- ✅ **Hecho (con tests):** catálogo de acciones, motor de políticas, papelera
  reversible 30 días, audit log, persistencia de configuración, kill switch.
- ⏳ **Siguiente (requiere permisos de macOS):** drivers reales —
  pantalla (Grabación), teclado/ratón (Accesibilidad), AppleScript/Automation
  (apps, Pages/Numbers, Mail), shell, navegador— **todos detrás del Governor**.
- ⏳ **UI:** panel de confirmaciones (HITL), visor de audit log, control de postura
  y kill switch, gestor de papelera.
