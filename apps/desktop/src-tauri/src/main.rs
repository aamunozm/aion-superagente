//! Shell de escritorio de AION (Tauri v2).
//!
//! Arranca el backend automáticamente como **sidecars** (núcleo + control-plane)
//! al abrir la app, y los detiene al cerrar. Un solo doble-clic, sin terminal.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::sync::Mutex;
use tauri::{Manager, RunEvent};
use tauri_plugin_shell::process::CommandChild;
use tauri_plugin_shell::ShellExt;

/// Procesos hijos (sidecars) para poder detenerlos al salir.
#[derive(Default)]
struct Sidecars(Mutex<Vec<CommandChild>>);

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .manage(Sidecars::default())
        .setup(|app| {
            let mut children = Vec::new();
            // Ollama embebido en un puerto PRIVADO (no choca con uno externo).
            let ollama_host = "127.0.0.1:11919";
            let ollama_url = format!("http://{ollama_host}");

            // 1) Motor LLM: Ollama EMBEBIDO (el usuario no instala nada).
            match app.shell().sidecar("ollama") {
                Ok(cmd) => match cmd.args(["serve"]).env("OLLAMA_HOST", ollama_host).spawn() {
                    Ok((_rx, child)) => children.push(child),
                    Err(e) => eprintln!("AION: no se pudo lanzar ollama embebido: {e}"),
                },
                Err(e) => eprintln!("AION: sidecar ollama no encontrado: {e}"),
            }

            // 2) Control-plane (auth/licencias) en :8787.
            match app.shell().sidecar("aion-control-plane") {
                Ok(cmd) => match cmd.spawn() {
                    Ok((_rx, child)) => children.push(child),
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
                    Ok((_rx, child)) => children.push(child),
                    Err(e) => eprintln!("AION: no se pudo lanzar el núcleo: {e}"),
                },
                Err(e) => eprintln!("AION: sidecar núcleo no encontrado: {e}"),
            }

            *app.state::<Sidecars>().0.lock().unwrap() = children;
            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("error al arrancar AION desktop")
        .run(|app, event| {
            // Al salir, detener los sidecars para no dejar procesos huérfanos.
            if let RunEvent::ExitRequested { .. } | RunEvent::Exit = event {
                if let Some(state) = app.try_state::<Sidecars>() {
                    for child in state.0.lock().unwrap().drain(..) {
                        let _ = child.kill();
                    }
                }
            }
        });
}
