//! Shell de escritorio de AION (Tauri v2).
//!
//! Arranca TODO el backend automáticamente al abrir la app y lo detiene al cerrar.
//! Un solo doble-clic, sin terminal y **sin instalar nada** (ni Docker ni Ollama):
//!
//! 1. **Motor LLM Ollama EMBEBIDO** — runtime portátil vendorizado en
//!    `Resources/ollama-runtime/` (binario universal arm64+x86_64 → Mac Silicon e
//!    Intel). Se lanza en un puerto PRIVADO para no chocar con un Ollama externo.
//! 2. **Control-plane** (auth/licencias) como sidecar.
//! 3. **Núcleo** (chat/agente/memoria) como sidecar, apuntando al Ollama embebido.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::process::Child as StdChild;
use std::sync::Mutex;
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{Manager, RunEvent, WindowEvent};
use tauri_plugin_shell::process::CommandChild;
use tauri_plugin_shell::ShellExt;

/// Puerto privado del Ollama embebido (poco común, evita choques con :11434).
const OLLAMA_HOST: &str = "127.0.0.1:11919";

/// Muestra (y enfoca) la ventana principal desde la bandeja o el icono del Dock.
/// En macOS restaura el icono del Dock (política Regular), que ocultamos al cerrar.
fn show_main(app: &tauri::AppHandle) {
    #[cfg(target_os = "macos")]
    let _ = app.set_activation_policy(tauri::ActivationPolicy::Regular);
    if let Some(win) = app.get_webview_window("main") {
        let _ = win.show();
        let _ = win.unminimize();
        let _ = win.set_focus();
    }
}

/// Procesos hijos para detenerlos limpiamente al salir.
#[derive(Default)]
struct Sidecars {
    /// Sidecars Tauri (núcleo + control-plane).
    tauri: Mutex<Vec<CommandChild>>,
    /// Ollama embebido (proceso del sistema lanzado desde el recurso).
    ollama: Mutex<Option<StdChild>>,
}

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .manage(Sidecars::default())
        .setup(|app| {
            let ollama_url = format!("http://{OLLAMA_HOST}");

            // 1) Motor LLM: Ollama EMBEBIDO desde Resources/ollama-runtime/.
            //    No es un sidecar Tauri porque necesita a su runner (llama-server +
            //    librerías nativas) como vecinos en la MISMA carpeta.
            let ollama_rel = if cfg!(windows) {
                "ollama-runtime/ollama.exe"
            } else {
                "ollama-runtime/ollama"
            };
            match app
                .path()
                .resolve(ollama_rel, tauri::path::BaseDirectory::Resource)
            {
                Ok(ollama_bin) => {
                    let mut ollama_cmd = std::process::Command::new(&ollama_bin);
                    ollama_cmd
                        .arg("serve")
                        .env("OLLAMA_HOST", OLLAMA_HOST)
                        // VELOCIDAD/MEMORIA sin perder calidad: flash-attention ON y la caché
                        // KV en q8_0 (la mitad de memoria, un poco más rápido, calidad ~igual).
                        .env("OLLAMA_FLASH_ATTENTION", "1")
                        .env("OLLAMA_KV_CACHE_TYPE", "q8_0");
                    // Unix: nuevo grupo de procesos (pgid = pid de ollama) para poder
                    // matar a ollama Y a sus runners llama-server de una sola vez.
                    #[cfg(unix)]
                    {
                        use std::os::unix::process::CommandExt;
                        ollama_cmd.process_group(0);
                    }
                    // Windows: SIN ventana de consola. Sin esto, ollama (y sus runners
                    // llama-server) heredan una consola y aparece una terminal negra.
                    #[cfg(windows)]
                    {
                        use std::os::windows::process::CommandExt;
                        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
                        ollama_cmd.creation_flags(CREATE_NO_WINDOW);
                    }
                    match ollama_cmd.spawn() {
                        Ok(child) => {
                            *app.state::<Sidecars>().ollama.lock().unwrap() = Some(child);
                            // Bootstrap de modelos en primer arranque (segundo plano,
                            // MULTIPLATAFORMA): el núcleo asegura los modelos con el
                            // binario ollama embebido. Idempotente si ya existen.
                            if let Ok(modelfile) = app.path().resolve(
                                "bootstrap/Modelfile.aion",
                                tauri::path::BaseDirectory::Resource,
                            ) {
                                if let Ok(cmd) = app.shell().sidecar("aion-core") {
                                    let _ = cmd
                                        .args(["models-ensure"])
                                        .env("OLLAMA_HOST", OLLAMA_HOST)
                                        .env(
                                            "AION_OLLAMA_BIN",
                                            ollama_bin.to_string_lossy().as_ref(),
                                        )
                                        .env("AION_MODELFILE", modelfile.to_string_lossy().as_ref())
                                        .spawn();
                                }
                            }
                        }
                        Err(e) => eprintln!("AION: no se pudo lanzar Ollama embebido: {e}"),
                    }
                }
                Err(e) => eprintln!("AION: no encuentro ollama-runtime en Resources: {e}"),
            }

            let mut tauri_children = Vec::new();

            // 2) Control-plane (auth/licencias) en :8787.
            match app.shell().sidecar("aion-control-plane") {
                Ok(cmd) => match cmd.spawn() {
                    Ok((_rx, child)) => tauri_children.push(child),
                    Err(e) => eprintln!("AION: no se pudo lanzar control-plane: {e}"),
                },
                Err(e) => eprintln!("AION: sidecar control-plane no encontrado: {e}"),
            }

            // 3) Núcleo (chat/agente/memoria) en :8765, apuntando al Ollama embebido.
            match app.shell().sidecar("aion-core") {
                Ok(cmd) => match cmd
                    .args(["serve", "127.0.0.1:8765"])
                    .env("AION_OLLAMA_URL", &ollama_url)
                    .spawn()
                {
                    Ok((_rx, child)) => tauri_children.push(child),
                    Err(e) => eprintln!("AION: no se pudo lanzar el núcleo: {e}"),
                },
                Err(e) => eprintln!("AION: sidecar núcleo no encontrado: {e}"),
            }

            *app.state::<Sidecars>().tauri.lock().unwrap() = tauri_children;

            // Abrir A PANTALLA COMPLETA sin parpadeo en dos etapas. La ventana nace
            // OCULTA (`"visible": false`) y con el color de fondo de la app
            // (`backgroundColor`), así el instante previo a pintar no es un fogonazo
            // blanco. En vez de `maximize()` —que en macOS puede ANIMAR pequeña→grande—
            // fijamos tamaño y posición al ÁREA DE TRABAJO del monitor MIENTRAS está
            // oculta (sin animación) y solo entonces la mostramos. Fallback a maximize().
            if let Some(win) = app.get_webview_window("main") {
                let sized = match win.current_monitor() {
                    Ok(Some(m)) => {
                        let wa = m.work_area();
                        win.set_position(wa.position).is_ok() && win.set_size(wa.size).is_ok()
                    }
                    _ => false,
                };
                if !sized {
                    let _ = win.maximize();
                }
                let _ = win.show();
                let _ = win.set_focus();

                // CERRAR LA VENTANA = OCULTAR A LA BARRA DE MENÚ (no salir). El proceso
                // sigue vivo, así que el núcleo aion-core (:8765) y con él Claude Code,
                // la memoria y el agente SIGUEN funcionando en background. Salir de verdad
                // se hace desde la bandeja ("Salir") o con ⌘Q.
                let handle = app.handle().clone();
                win.on_window_event(move |event| {
                    if let WindowEvent::CloseRequested { api, .. } = event {
                        api.prevent_close();
                        if let Some(w) = handle.get_webview_window("main") {
                            let _ = w.hide();
                        }
                        // macOS: quita el icono del Dock → queda SOLO en la barra de menú.
                        #[cfg(target_os = "macos")]
                        let _ = handle.set_activation_policy(tauri::ActivationPolicy::Accessory);
                    }
                });
            }

            // BANDEJA (barra de menú): AION queda "arriba" aunque cierres la ventana.
            // Icono con menú "Mostrar aplicación" / "Salir". Clic izquierdo → mostrar.
            if let Some(icon) = app.default_window_icon().cloned() {
                let show_i =
                    MenuItem::with_id(app, "show", "Mostrar aplicación", true, None::<&str>)?;
                let sep = PredefinedMenuItem::separator(app)?;
                let quit_i = MenuItem::with_id(app, "quit", "Salir", true, None::<&str>)?;
                let menu = Menu::with_items(app, &[&show_i, &sep, &quit_i])?;
                let _tray = TrayIconBuilder::with_id("aion-tray")
                    .icon(icon)
                    .tooltip("AION")
                    .menu(&menu)
                    .show_menu_on_left_click(false)
                    .on_menu_event(|app, event| match event.id.as_ref() {
                        "show" => show_main(app),
                        // "Salir": cierre DEFINITIVO. Dispara RunEvent::Exit, que mata los
                        // sidecars (aion-core, control-plane, ollama) limpiamente.
                        "quit" => app.exit(0),
                        _ => {}
                    })
                    .on_tray_icon_event(|tray, event| {
                        if let TrayIconEvent::Click {
                            button: MouseButton::Left,
                            button_state: MouseButtonState::Up,
                            ..
                        } = event
                        {
                            show_main(tray.app_handle());
                        }
                    })
                    .build(app)?;
            }
            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("error al arrancar AION desktop")
        .run(|app, event| {
            // Al salir, detener TODOS los procesos para no dejar huérfanos.
            if let RunEvent::ExitRequested { .. } | RunEvent::Exit = event {
                if let Some(state) = app.try_state::<Sidecars>() {
                    for child in state.tauri.lock().unwrap().drain(..) {
                        let _ = child.kill();
                    }
                    if let Some(mut ollama) = state.ollama.lock().unwrap().take() {
                        // Unix: SIGTERM a TODO el grupo (ollama + sus runners
                        // llama-server) para un apagado limpio sin huérfanos.
                        #[cfg(unix)]
                        {
                            let pid = ollama.id() as i32;
                            unsafe {
                                libc::kill(-pid, libc::SIGTERM);
                            }
                        }
                        let _ = ollama.kill();
                    }
                }
            }
        });
}
