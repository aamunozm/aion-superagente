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
    /// Atajo/combo: modificadores + tecla final (p. ej. cmd+s, ctrl+shift+t).
    /// Mantiene los modificadores pulsados, pulsa la tecla y los suelta.
    Chord { mods: Vec<String>, key: String },
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
            ControlIntent::Chord { mods, key } => {
                format!("Atajo: {}+{}", mods.join("+"), key)
            }
        }
    }
}

/// Ejecuta una intención de control sobre el SO. Solo lo invoca `Computer` tras
/// autorización del Governor.
pub(crate) fn execute(intent: &ControlIntent) -> Result<(), ControlError> {
    let mut enigo = Enigo::new(&Settings::default()).map_err(|e| {
        ControlError::Input(format!(
            "falta el permiso de Accesibilidad. Actívalo en Ajustes del Sistema → Privacidad y \
             seguridad → Accesibilidad → activar AION, y reabre AION. ({e})"
        ))
    })?;

    let click_at = |enigo: &mut Enigo,
                    x: i32,
                    y: i32,
                    button: Button,
                    times: u32|
     -> Result<(), ControlError> {
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
        ControlIntent::Chord { mods, key } => {
            let mod_keys: Vec<Key> = mods
                .iter()
                .map(|m| parse_modifier(m))
                .collect::<Result<_, _>>()?;
            let main = parse_key(key)?;
            // Mantén pulsados los modificadores, pulsa la tecla, suéltalos en orden inverso.
            for &mk in &mod_keys {
                enigo
                    .key(mk, Direction::Press)
                    .map_err(|e| ControlError::Input(format!("modificador: {e}")))?;
            }
            let click = enigo.key(main, Direction::Click);
            for &mk in mod_keys.iter().rev() {
                let _ = enigo.key(mk, Direction::Release);
            }
            click.map_err(|e| ControlError::Input(format!("atajo: {e}")))?;
        }
    }
    Ok(())
}

/// Modificador por nombre. `cmd`/`meta`/`win` → Meta (⌘ en macOS).
fn parse_modifier(name: &str) -> Result<Key, ControlError> {
    let k = match name.to_lowercase().as_str() {
        "cmd" | "command" | "meta" | "win" | "super" => Key::Meta,
        "ctrl" | "control" => Key::Control,
        "alt" | "option" | "opt" => Key::Alt,
        "shift" => Key::Shift,
        other => {
            return Err(ControlError::Input(format!(
                "modificador no soportado: {other}"
            )))
        }
    };
    Ok(k)
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
            // Carácter suelto (letra/dígito/símbolo): p. ej. "s" en cmd+s.
            let mut chars = other.chars();
            match (chars.next(), chars.next()) {
                (Some(c), None) => Key::Unicode(c),
                _ => {
                    return Err(ControlError::Input(format!("tecla no soportada: {other}")));
                }
            }
        }
    };
    Ok(k)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_key_named_and_single_char() {
        assert!(matches!(parse_key("enter").unwrap(), Key::Return));
        assert!(matches!(parse_key("s").unwrap(), Key::Unicode('s')));
        assert!(matches!(parse_key("7").unwrap(), Key::Unicode('7')));
        // Cadena ambigua (varios caracteres y no es un nombre conocido) → error.
        assert!(parse_key("noexiste").is_err());
    }

    #[test]
    fn parse_modifier_aliases() {
        assert!(matches!(parse_modifier("cmd").unwrap(), Key::Meta));
        assert!(matches!(parse_modifier("Command").unwrap(), Key::Meta));
        assert!(matches!(parse_modifier("ctrl").unwrap(), Key::Control));
        assert!(matches!(parse_modifier("option").unwrap(), Key::Alt));
        assert!(matches!(parse_modifier("shift").unwrap(), Key::Shift));
        assert!(parse_modifier("hyper").is_err());
    }

    #[test]
    fn chord_summary_reads_well() {
        let c = ControlIntent::Chord {
            mods: vec!["cmd".into(), "shift".into()],
            key: "t".into(),
        };
        assert_eq!(c.summary(), "Atajo: cmd+shift+t");
        assert_eq!(crate::verb_for(&c), "ui.chord");
    }
}
