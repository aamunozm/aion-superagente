//! **Lectura NATIVA del árbol de accesibilidad** de la app en primer plano, por sistema.
//!
//! macOS usa la AX API (rápida + desbloqueo de Electron vía `AXManualAccessibility`);
//! Windows usa UI Automation (UIA); otros SO devuelven vacío (el llamador usa su
//! fallback). Devuelve elementos interactivos con rol, etiqueta y centro (x,y). 100% Rust.

/// Un elemento interactivo de la UI con su posición central en pantalla.
#[derive(Debug, Clone)]
pub struct UiEl {
    pub role: String,
    pub name: String,
    pub x: i32,
    pub y: i32,
}

/// Desbloquea el árbol de accesibilidad de la app frontal (Electron/Chromium suelen
/// venir vacíos hasta que se pide). No-op donde no aplica.
pub fn unlock_frontmost() {
    #[cfg(target_os = "macos")]
    macos::unlock_frontmost();
}

/// Elementos interactivos de la ventana en primer plano (vacío si no hay backend).
pub fn frontmost_elements(max: usize) -> Vec<UiEl> {
    #[cfg(target_os = "macos")]
    {
        return macos::frontmost_elements(max);
    }
    #[cfg(target_os = "windows")]
    {
        return windows::frontmost_elements(max);
    }
    #[allow(unreachable_code)]
    {
        let _ = max;
        Vec::new()
    }
}

// ── macOS: AX API nativa ─────────────────────────────────────────────────────
#[cfg(target_os = "macos")]
mod macos {
    use super::UiEl;
    use accessibility::{AXAttribute, AXUIElement, AXUIElementAttributes};
    use accessibility_sys::{kAXValueTypeCGPoint, kAXValueTypeCGSize, AXValueGetValue, AXValueRef};
    use core_foundation::base::{CFType, TCFType};
    use core_foundation::boolean::CFBoolean;
    use core_foundation::string::CFString;

    #[repr(C)]
    #[derive(Clone, Copy)]
    struct CGPoint {
        x: f64,
        y: f64,
    }
    #[repr(C)]
    #[derive(Clone, Copy)]
    struct CGSize {
        w: f64,
        h: f64,
    }

    /// Roles interactivos que nos interesan (los demás se ignoran).
    fn is_interactive(role: &str) -> bool {
        matches!(
            role,
            "AXButton"
                | "AXTextField"
                | "AXTextArea"
                | "AXCheckBox"
                | "AXRadioButton"
                | "AXPopUpButton"
                | "AXMenuButton"
                | "AXComboBox"
                | "AXLink"
                | "AXTabButton"
                | "AXDisclosureTriangle"
                | "AXMenuItem"
                | "AXSlider"
        )
    }

    /// PID del proceso en primer plano (una llamada barata a System Events). El
    /// elemento system-wide `AXFocusedApplication` da `cannotComplete` a menudo, así que
    /// usamos el PID y creamos el elemento de app nativo (robusto y rápido).
    fn frontmost_pid() -> Option<i32> {
        let out = std::process::Command::new("osascript")
            .arg("-e")
            .arg("tell application \"System Events\" to get unix id of first process whose frontmost is true")
            .output()
            .ok()?;
        String::from_utf8_lossy(&out.stdout).trim().parse().ok()
    }

    fn focused_app() -> Option<AXUIElement> {
        Some(AXUIElement::application(frontmost_pid()?))
    }

    pub fn unlock_frontmost() {
        if let Some(app) = focused_app() {
            // AXManualAccessibility=true fuerza a Electron/Chromium a exponer su árbol.
            let attr = AXAttribute::<CFType>::new(&CFString::new("AXManualAccessibility"));
            let _ = app.set_attribute(&attr, CFBoolean::true_value().as_CFType());
        }
    }

    fn cg_point(el: &AXUIElement) -> Option<(f64, f64)> {
        let attr = AXAttribute::<CFType>::new(&CFString::new("AXPosition"));
        let v = el.attribute(&attr).ok()?;
        let mut p = CGPoint { x: 0.0, y: 0.0 };
        let ok = unsafe {
            AXValueGetValue(
                v.as_CFTypeRef() as AXValueRef,
                kAXValueTypeCGPoint,
                &mut p as *mut _ as *mut std::ffi::c_void,
            )
        };
        ok.then_some((p.x, p.y))
    }

    fn cg_size(el: &AXUIElement) -> Option<(f64, f64)> {
        let attr = AXAttribute::<CFType>::new(&CFString::new("AXSize"));
        let v = el.attribute(&attr).ok()?;
        let mut s = CGSize { w: 0.0, h: 0.0 };
        let ok = unsafe {
            AXValueGetValue(
                v.as_CFTypeRef() as AXValueRef,
                kAXValueTypeCGSize,
                &mut s as *mut _ as *mut std::ffi::c_void,
            )
        };
        ok.then_some((s.w, s.h))
    }

    fn clip(s: String) -> String {
        s.trim().chars().take(80).collect()
    }

    fn label(el: &AXUIElement) -> String {
        // Preferimos título; si no, descripción; si no, valor (texto).
        if let Ok(t) = el.title() {
            if !t.to_string().trim().is_empty() {
                return clip(t.to_string());
            }
        }
        if let Ok(d) = el.description() {
            if !d.to_string().trim().is_empty() {
                return clip(d.to_string());
            }
        }
        if let Ok(v) = el.value() {
            if let Some(s) = v.downcast::<CFString>() {
                if !s.to_string().trim().is_empty() {
                    return clip(s.to_string());
                }
            }
        }
        String::new()
    }

    fn role(el: &AXUIElement) -> String {
        el.role().map(|s| s.to_string()).unwrap_or_default()
    }

    fn children(el: &AXUIElement) -> Vec<AXUIElement> {
        match el.children() {
            Ok(arr) => arr.iter().map(|c| c.clone()).collect(),
            Err(_) => Vec::new(),
        }
    }

    fn walk(el: &AXUIElement, depth: usize, out: &mut Vec<UiEl>, max: usize) {
        if depth > 12 || out.len() >= max {
            return;
        }
        let r = role(el);
        if is_interactive(&r) {
            if let (Some((px, py)), Some((sw, sh))) = (cg_point(el), cg_size(el)) {
                if sw > 0.0 && sh > 0.0 {
                    out.push(UiEl {
                        role: r.trim_start_matches("AX").to_lowercase(),
                        name: label(el),
                        x: (px + sw / 2.0) as i32,
                        y: (py + sh / 2.0) as i32,
                    });
                }
            }
        }
        for c in children(el) {
            walk(&c, depth + 1, out, max);
            if out.len() >= max {
                break;
            }
        }
    }

    pub fn frontmost_elements(max: usize) -> Vec<UiEl> {
        let Some(app) = focused_app() else {
            return Vec::new();
        };
        // Desbloquea (Electron) antes de leer.
        let attr = AXAttribute::<CFType>::new(&CFString::new("AXManualAccessibility"));
        let _ = app.set_attribute(&attr, CFBoolean::true_value().as_CFType());

        let mut out = Vec::new();
        // Recorre las ventanas de la app (accessor tipado).
        if let Ok(arr) = app.windows() {
            for w in arr.iter() {
                walk(&w, 0, &mut out, max);
                if out.len() >= max {
                    break;
                }
            }
        }
        out
    }
}

// ── Windows: UI Automation (UIA) ─────────────────────────────────────────────
#[cfg(target_os = "windows")]
mod windows {
    use super::UiEl;
    use uiautomation::UIAutomation;

    pub fn frontmost_elements(max: usize) -> Vec<UiEl> {
        let Ok(auto) = UIAutomation::new() else {
            return Vec::new();
        };
        // Ventana en primer plano.
        let Ok(root) = auto
            .get_focused_element()
            .or_else(|_| auto.get_root_element())
        else {
            return Vec::new();
        };
        // Recorre con un walker de control (solo elementos de control relevantes).
        let Ok(walker) = auto.get_control_view_walker() else {
            return Vec::new();
        };
        let mut out = Vec::new();
        walk(&walker, &root, 0, &mut out, max);
        out
    }

    fn walk(
        walker: &uiautomation::UITreeWalker,
        el: &uiautomation::UIElement,
        depth: usize,
        out: &mut Vec<UiEl>,
        max: usize,
    ) {
        if depth > 12 || out.len() >= max {
            return;
        }
        // Tipo de control legible + nombre + rectángulo.
        if let Ok(ct) = el.get_control_type() {
            let role = format!("{ct:?}").to_lowercase();
            let interactive = matches!(
                role.as_str(),
                s if s.contains("button")
                    || s.contains("edit")
                    || s.contains("checkbox")
                    || s.contains("radio")
                    || s.contains("combobox")
                    || s.contains("hyperlink")
                    || s.contains("menuitem")
                    || s.contains("tab")
                    || s.contains("list")
            );
            if interactive {
                if let Ok(rect) = el.get_bounding_rectangle() {
                    // Rect expone left/top/right/bottom; el ancho/alto se derivan.
                    let w = rect.get_right() - rect.get_left();
                    let h = rect.get_bottom() - rect.get_top();
                    if w > 0 && h > 0 {
                        out.push(UiEl {
                            role,
                            name: el.get_name().unwrap_or_default().chars().take(80).collect(),
                            x: rect.get_left() + w / 2,
                            y: rect.get_top() + h / 2,
                        });
                    }
                }
            }
        }
        // Hijos.
        if let Ok(mut child) = walker.get_first_child(el) {
            loop {
                walk(walker, &child, depth + 1, out, max);
                if out.len() >= max {
                    break;
                }
                match walker.get_next_sibling(&child) {
                    Ok(next) => child = next,
                    Err(_) => break,
                }
            }
        }
    }
}
