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

            // Control-plane (auth/licencias) en :8787.
            match app.shell().sidecar("aion-control-plane") {
                Ok(cmd) => match cmd.spawn() {
                    Ok((_rx, child)) => children.push(child),
                    Err(e) => eprintln!("AION: no se pudo lanzar control-plane: {e}"),
                },
                Err(e) => eprintln!("AION: sidecar control-plane no encontrado: {e}"),
            }

            // Núcleo (chat/agente/memoria) como puente HTTP en :8765.
            match app.shell().sidecar("aion-core") {
                Ok(cmd) => match cmd.args(["serve", "127.0.0.1:8765"]).spawn() {
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
