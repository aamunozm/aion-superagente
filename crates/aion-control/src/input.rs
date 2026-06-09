//! Driver de **entrada**: ratón y teclado, multiplataforma (vía `enigo`: Quartz en
//! macOS, Win32 en Windows, libei/X11 en Linux). En macOS requiere permiso de
//! "Accesibilidad". Estas primitivas NUNCA se llaman directamente: el [`crate::Computer`]
//! las ejecuta solo después de que el Governor autorice la acción.

use crate::ControlError;
use enigo::{Button, Coordinate, Direction, Enigo, Key, Keyboard, Mouse, Settings};

/// Intención de control de bajo nivel que el agente quiere realizar.
#[derive(Debug, Clone)]
pub enum ControlIntent {
    /// Mover el ratón a coordenadas absolutas y hacer clic izquierdo.
    Click { x: i32, y: i32 },
    /// Doble clic en coordenadas absolutas.
    DoubleClick { x: i32, y: i32 },
    /// Clic derecho.
    RightClick { x: i32, y: i32 },
    /// Escribir texto.
    Type { text: String },
    /// Pulsar una tecla con nombre (enter, tab, esc, space, delete, arrow…).
    Key { name: String },
}

impl ControlIntent {
    /// Descripción legible (para la confirmación HITL y el audit log).
    pub fn summary(&self) -> String {
        match self {
            ControlIntent::Click { x, y } => format!("Clic en ({x}, {y})"),
            ControlIntent::DoubleClick { x, y } => format!("Doble clic en ({x}, {y})"),
            ControlIntent::RightClick { x, y } => format!("Clic derecho en ({x}, {y})"),
            ControlIntent::Type { text } => format!("Escribir: «{text}»"),
            ControlIntent::Key { name } => format!("Pulsar tecla: {name}"),
        }
    }
}

/// Ejecuta una intención de control sobre el SO. Solo lo invoca `Computer` tras
/// autorización del Governor.
pub(crate) fn execute(intent: &ControlIntent) -> Result<(), ControlError> {
    let mut enigo = Enigo::new(&Settings::default())
        .map_err(|e| ControlError::Input(format!("no se pudo iniciar el control de entrada (¿falta permiso de Accesibilidad?): {e}")))?;

    let click_at = |enigo: &mut Enigo, x: i32, y: i32, button: Button, times: u32| -> Result<(), ControlError> {
        enigo
            .move_mouse(x, y, Coordinate::Abs)
            .map_err(|e| ControlError::Input(format!("mover ratón: {e}")))?;
        for _ in 0..times {
            enigo
                .button(button, Direction::Click)
                .map_err(|e| ControlError::Input(format!("clic: {e}")))?;
        }
        Ok(())
    };

    match intent {
        ControlIntent::Click { x, y } => click_at(&mut enigo, *x, *y, Button::Left, 1)?,
        ControlIntent::DoubleClick { x, y } => click_at(&mut enigo, *x, *y, Button::Left, 2)?,
        ControlIntent::RightClick { x, y } => click_at(&mut enigo, *x, *y, Button::Right, 1)?,
        ControlIntent::Type { text } => enigo
            .text(text)
            .map_err(|e| ControlError::Input(format!("escribir: {e}")))?,
        ControlIntent::Key { name } => {
            let key = parse_key(name)?;
            enigo
                .key(key, Direction::Click)
                .map_err(|e| ControlError::Input(format!("tecla: {e}")))?;
        }
    }
    Ok(())
}

fn parse_key(name: &str) -> Result<Key, ControlError> {
    let k = match name.to_lowercase().as_str() {
        "enter" | "return" => Key::Return,
        "tab" => Key::Tab,
        "esc" | "escape" => Key::Escape,
        "space" => Key::Space,
        "delete" | "backspace" => Key::Backspace,
        "up" => Key::UpArrow,
        "down" => Key::DownArrow,
        "left" => Key::LeftArrow,
        "right" => Key::RightArrow,
        other => {
            return Err(ControlError::Input(format!("tecla no soportada: {other}")));
        }
    };
    Ok(k)
}
