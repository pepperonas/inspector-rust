//! Linux desktop integration: register system shortcuts when Tauri global
//! shortcuts do not work (GNOME + Wayland). Uses gsettings on GNOME/Cinnamon.
//!
//! On first start we **scan** existing bindings (custom shortcuts + terminal
//! profiles), resolve collisions automatically, and only then install ours —
//! no manual “change your terminal to Ctrl+C” step.

#[cfg(target_os = "linux")]
mod imp {
    use anyhow::{Context, Result};
    use serde::Serialize;
    use std::collections::HashSet;
    use std::process::Command;

    use crate::{db::DbHandle, settings};

    const SETTINGS_KEY: &str = "linux.desktop_shortcuts_profile";
    const SETTINGS_BINDINGS_KEY: &str = "linux.desktop_shortcuts_bindings";

    const GNOME_MEDIA_KEYS: &str = "org.gnome.settings-daemon.plugins.media-keys";
    const GNOME_CUSTOM_SCHEMA: &str =
        "org.gnome.settings-daemon.plugins.media-keys.custom-keybinding";

    const CINNAMON_KEYBINDINGS: &str = "org.cinnamon.desktop.keybindings";
    const CINNAMON_CUSTOM_SCHEMA: &str = "org.cinnamon.keybindings.custom-keybinding";

    /// Normalize a GNOME accel (`'<Control><Shift>c'`) to `control+shift+c` for comparison.
    pub(crate) fn normalize_binding(raw: &str) -> String {
        let s = raw.trim().trim_matches('\'').to_lowercase();
        if s.is_empty() {
            return String::new();
        }
        if !s.contains('<') {
            return s;
        }
        let mut mods = Vec::new();
        let mut key = String::new();
        let mut rest = s.as_str();
        while let Some(start) = rest.find('<') {
            if start > 0 {
                key = rest[..start].to_string();
            }
            rest = &rest[start + 1..];
            let Some(end) = rest.find('>') else { break };
            match &rest[..end] {
                "control" | "ctrl" => mods.push("control"),
                "shift" => mods.push("shift"),
                "alt" => mods.push("alt"),
                "super" => mods.push("super"),
                other if !other.is_empty() => key = other.to_string(),
                _ => {}
            }
            rest = &rest[end + 1..];
        }
        if !rest.is_empty() {
            key = rest.to_string();
        }
        mods.sort_unstable();
        mods.dedup();
        if key.is_empty() && mods.is_empty() {
            return String::new();
        }
        if key.is_empty() {
            mods.join("+")
        } else if mods.is_empty() {
            key
        } else {
            format!("{}+{}", mods.join("+"), key)
        }
    }

    pub(crate) fn bindings_conflict(a: &str, b: &str) -> bool {
        let na = normalize_binding(a);
        let nb = normalize_binding(b);
        !na.is_empty() && na == nb
    }

    #[derive(Debug, Clone)]
    struct ShortcutSpec {
        id: &'static str,
        name: &'static str,
        arg: &'static str,
        /// Preferred binding first, then fallbacks (auto-picked on collision).
        binding_candidates: &'static [&'static str],
    }

    const SHORTCUTS: &[ShortcutSpec] = &[
        ShortcutSpec {
            id: "toggle",
            name: "Inspector Rust — Open",
            arg: "--toggle-popup",
            binding_candidates: &[
                "<Control><Shift>v",
                "<Control><Alt>v",
                "<Super><Shift>v",
                "<Control><Shift>Insert",
            ],
        },
        ShortcutSpec {
            id: "ocr",
            name: "Inspector Rust — OCR",
            arg: "--ocr",
            binding_candidates: &["<Control><Shift>o", "<Control><Alt>o", "<Super><Shift>o"],
        },
        ShortcutSpec {
            id: "screenshot",
            name: "Inspector Rust — Screenshot",
            arg: "--screenshot",
            binding_candidates: &["<Control><Shift>s", "<Control><Alt>s", "<Super><Shift>s"],
        },
        ShortcutSpec {
            id: "color",
            name: "Inspector Rust — Pick color",
            arg: "--pick-color",
            binding_candidates: &["<Control><Shift>c", "<Control><Alt>c", "<Super><Shift>c"],
        },
    ];

    /// GNOME Terminal defaults — Inspector must **not** steal these; pick fallbacks instead.
    const TERMINAL_SHIFT_COPY: &str = "<Control><Shift>c";
    const TERMINAL_SHIFT_PASTE: &str = "<Control><Shift>v";
    /// Broken state from an earlier Inspector build that wrongly moved Terminal here.
    /// In terminals Ctrl+C is SIGINT, not copy — never assign these as copy/paste.
    const TERMINAL_BROKEN_COPY: &str = "<Control>c";
    const TERMINAL_BROKEN_PASTE: &str = "<Control>v";

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum LinuxDesktop {
        X11,
        GnomeWayland,
        CinnamonWayland,
        Kde,
        Unknown,
    }

    pub fn detect() -> LinuxDesktop {
        let session = std::env::var("XDG_SESSION_TYPE")
            .unwrap_or_default()
            .to_lowercase();
        let desktop = std::env::var("XDG_CURRENT_DESKTOP")
            .unwrap_or_default()
            .to_lowercase();

        if session == "x11" {
            return LinuxDesktop::X11;
        }

        if desktop.contains("kde") || desktop.contains("plasma") {
            return LinuxDesktop::Kde;
        }

        if desktop.contains("cinnamon") || desktop.contains("x-cinnamon") {
            if session == "wayland" || std::env::var_os("WAYLAND_DISPLAY").is_some() {
                return LinuxDesktop::CinnamonWayland;
            }
            return LinuxDesktop::X11;
        }

        if desktop.contains("gnome")
            || desktop.contains("ubuntu")
            || desktop.contains("unity")
            || desktop.is_empty()
        {
            if session == "wayland" || std::env::var_os("WAYLAND_DISPLAY").is_some() {
                return LinuxDesktop::GnomeWayland;
            }
            return LinuxDesktop::X11;
        }

        if std::env::var_os("WAYLAND_DISPLAY").is_some() {
            return LinuxDesktop::Unknown;
        }

        LinuxDesktop::X11
    }

    fn inspector_command() -> String {
        if let Ok(p) = which_inspector_rust() {
            return p;
        }
        "/usr/bin/inspector-rust".into()
    }

    fn which_inspector_rust() -> Result<String> {
        let output = Command::new("which")
            .arg("inspector-rust")
            .output()
            .context("which inspector-rust")?;
        if !output.status.success() {
            anyhow::bail!("inspector-rust not in PATH");
        }
        let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if path.is_empty() {
            anyhow::bail!("empty path from which");
        }
        Ok(path)
    }

    fn gsettings(args: &[&str]) -> Result<String> {
        let output = Command::new("gsettings")
            .args(args)
            .output()
            .with_context(|| format!("gsettings {}", args.join(" ")))?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            anyhow::bail!("gsettings failed: {stderr}");
        }
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    }

    fn gsettings_set(args: &[&str]) -> Result<()> {
        let status = Command::new("gsettings")
            .args(args)
            .status()
            .with_context(|| format!("gsettings set {}", args[2..].join(" ")))?;
        if !status.success() {
            anyhow::bail!("gsettings set failed");
        }
        Ok(())
    }

    fn parse_gsettings_array(raw: &str) -> Vec<String> {
        let trimmed = raw.trim();
        if trimmed.is_empty() || trimmed == "@as []" {
            return Vec::new();
        }
        let inner = trimmed.strip_prefix("@as ").unwrap_or(trimmed).trim();
        inner
            .trim_matches(|c| c == '[' || c == ']')
            .split(',')
            .map(|s| s.trim().trim_matches('\'').to_string())
            .filter(|s| !s.is_empty())
            .collect()
    }

    fn format_gsettings_array(paths: &[String]) -> String {
        if paths.is_empty() {
            return "@as []".into();
        }
        let inner: Vec<String> = paths.iter().map(|p| format!("'{p}'")).collect();
        format!("@as [{}]", inner.join(", "))
    }

    fn remove_our_paths(paths: &mut Vec<String>, prefix: &str) {
        paths.retain(|p| !p.contains(prefix));
    }

    /// Read all non–Inspector-Rust custom shortcut bindings from gsettings.
    fn collect_custom_shortcut_bindings(
        list_schema: &str,
        list_key: &str,
        custom_schema: &str,
        our_prefix: &str,
    ) -> Result<HashSet<String>> {
        let mut occupied = HashSet::new();
        let paths = parse_gsettings_array(&gsettings(&["get", list_schema, list_key])?);
        for path in paths {
            if path.contains(our_prefix) {
                continue;
            }
            let schema = format!("{custom_schema}:{path}");
            let binding = gsettings(&["get", &schema, "binding"]).unwrap_or_default();
            let norm = normalize_binding(&binding);
            if !norm.is_empty() {
                occupied.insert(norm);
            }
        }
        Ok(occupied)
    }

    /// Terminal copy/paste keys that would collide with Inspector defaults.
    fn collect_terminal_bindings() -> Result<HashSet<String>> {
        let mut occupied = HashSet::new();
        let list = gsettings(&["get", "org.gnome.Terminal.ProfilesList", "list"])?;
        for uuid in parse_gsettings_array(&list) {
            let schema = format!(
                "org.gnome.Terminal.Legacy.Keybindings:/org/gnome/terminal/legacy/profiles:/{uuid}/"
            );
            for key in ["copy", "paste"] {
                if let Ok(binding) = gsettings(&["get", &schema, key]) {
                    let norm = normalize_binding(&binding);
                    if !norm.is_empty() {
                        occupied.insert(norm);
                    }
                }
            }
        }
        Ok(occupied)
    }

    /// Restore GNOME Terminal copy/paste to Ctrl+Shift+C/V when a previous
    /// Inspector build wrongly moved them to Ctrl+C/V (SIGINT / broken paste).
    /// Idempotent — safe to call on every startup.
    pub(crate) fn restore_terminal_copy_paste() -> Result<u32> {
        let list = gsettings(&["get", "org.gnome.Terminal.ProfilesList", "list"])?;
        let mut changed = 0u32;
        for uuid in parse_gsettings_array(&list) {
            let schema = format!(
                "org.gnome.Terminal.Legacy.Keybindings:/org/gnome/terminal/legacy/profiles:/{uuid}/"
            );
            let copy = gsettings(&["get", &schema, "copy"]).unwrap_or_default();
            let paste = gsettings(&["get", &schema, "paste"]).unwrap_or_default();
            let needs_copy = bindings_conflict(&copy, TERMINAL_BROKEN_COPY)
                || bindings_conflict(&copy, "<Control>C");
            let needs_paste = bindings_conflict(&paste, TERMINAL_BROKEN_PASTE)
                || bindings_conflict(&paste, "<Control>V");
            if needs_copy || needs_paste {
                if needs_copy {
                    gsettings_set(&["set", &schema, "copy", TERMINAL_SHIFT_COPY])?;
                }
                if needs_paste {
                    gsettings_set(&["set", &schema, "paste", TERMINAL_SHIFT_PASTE])?;
                }
                changed += 1;
                tracing::info!(
                    "Terminal profile {uuid}: restored copy/paste to Ctrl+Shift+C / Ctrl+Shift+V"
                );
            }
        }
        Ok(changed)
    }

    /// After restoring Terminal keys, move any Inspector shortcut that still
    /// collides with Terminal copy/paste (Ctrl+Shift+C/V) to the next free preset.
    pub fn restore_terminal_and_fix_shortcut_conflicts(db: &DbHandle) -> Result<()> {
        let restored = restore_terminal_copy_paste()?;
        if restored > 0 {
            tracing::info!(
                "Restored {restored} GNOME Terminal profile(s) — Ctrl+Shift+C/V is copy/paste again"
            );
        }

        let desktop = detect();
        let Some((list_key, list_schema, custom_schema, path_prefix, id_prefix)) =
            gnome_family_config(desktop)
        else {
            return Ok(());
        };

        let terminal_occupied = collect_terminal_bindings()?;
        let installed = read_our_installed_bindings(
            list_schema,
            list_key,
            custom_schema,
            path_prefix,
            id_prefix,
        )?;
        if installed.is_empty() {
            return Ok(());
        }

        let mut occupied =
            collect_custom_shortcut_bindings(list_schema, list_key, custom_schema, id_prefix)?;
        occupied.extend(terminal_occupied.iter().cloned());

        let mut reserved = HashSet::new();
        let mut resolved: Vec<(String, String)> = Vec::new();
        let mut changed = false;

        for spec in SHORTCUTS {
            let current = installed
                .iter()
                .find(|(id, _)| id == spec.id)
                .map(|(_, b)| b.clone())
                .unwrap_or_else(|| spec.binding_candidates[0].to_string());

            let norm = normalize_binding(&current);
            let collides = terminal_occupied.contains(&norm);

            let chosen = if collides || norm.is_empty() {
                pick_binding(spec.binding_candidates, &occupied, &reserved).with_context(|| {
                    format!("no free binding for {} after Terminal restore", spec.name)
                })?
            } else {
                current.clone()
            };

            let chosen_norm = normalize_binding(&chosen);
            if chosen_norm != norm {
                changed = true;
                tracing::info!(
                    "{} moved {} → {} (Terminal reserves Ctrl+Shift+C/V)",
                    spec.name,
                    binding_label(&current),
                    binding_label(&chosen)
                );
            }

            if !chosen_norm.is_empty() {
                reserved.insert(chosen_norm.clone());
                occupied.insert(chosen_norm);
            }

            let path = format!("{path_prefix}{id_prefix}{}/", spec.id);
            let schema = format!("{custom_schema}:{path}");
            gsettings_set(&["set", &schema, "binding", &chosen])?;
            resolved.push((spec.id.to_string(), chosen));
        }

        if changed {
            let summary: String = resolved
                .iter()
                .map(|(id, b)| format!("{id}={b}"))
                .collect::<Vec<_>>()
                .join(",");
            settings::set(db, SETTINGS_BINDINGS_KEY, &summary)?;
            tracing::info!("Updated Inspector desktop shortcuts to avoid Terminal copy/paste");
        }

        Ok(())
    }

    fn pick_binding(
        candidates: &[&str],
        occupied: &HashSet<String>,
        reserved: &HashSet<String>,
    ) -> Option<String> {
        for &cand in candidates {
            let norm = normalize_binding(cand);
            if norm.is_empty() {
                continue;
            }
            if occupied.contains(&norm) || reserved.contains(&norm) {
                continue;
            }
            return Some(cand.to_string());
        }
        None
    }

    pub(crate) fn binding_label(raw: &str) -> String {
        let norm = normalize_binding(raw);
        let parts: Vec<&str> = norm.split('+').collect();
        if parts.is_empty() {
            return norm;
        }
        let key = parts.last().unwrap_or(&"");
        let mods: Vec<String> = parts[..parts.len().saturating_sub(1)]
            .iter()
            .map(|m| match *m {
                "control" => "Ctrl".into(),
                "shift" => "Shift".into(),
                "alt" => "Alt".into(),
                "super" => "Super".into(),
                other => other.to_string(),
            })
            .collect();
        if mods.is_empty() {
            key.to_string()
        } else {
            format!("{}+{}", mods.join("+"), key)
        }
    }

    /// Convert W3C hotkey (`Ctrl+Shift+KeyV`) to GNOME accel (`<Control><Shift>v`).
    pub fn web_hotkey_to_gsettings(sc: &str) -> Result<String, String> {
        let trimmed = sc.trim();
        if trimmed.is_empty() {
            return Err("empty shortcut".into());
        }
        if trimmed.starts_with('<') {
            return Ok(trimmed.to_string());
        }
        let parts: Vec<&str> = trimmed.split('+').collect();
        let mut mods = Vec::new();
        let mut key = String::new();
        for p in parts {
            match p {
                "Ctrl" | "Control" => mods.push("<Control>"),
                "Shift" => mods.push("<Shift>"),
                "Alt" => mods.push("<Alt>"),
                "Meta" | "Super" | "Win" => mods.push("<Super>"),
                s if s.starts_with("Key") && s.len() == 4 => {
                    key = s[3..].to_lowercase();
                }
                s if s.starts_with("Digit") && s.len() == 6 => {
                    key = s[5..].to_string();
                }
                "Insert" => key = "Insert".into(),
                "Delete" => key = "Delete".into(),
                "Home" => key = "Home".into(),
                "End" => key = "End".into(),
                "PageUp" => key = "Page_Up".into(),
                "PageDown" => key = "Page_Down".into(),
                "Space" => key = "space".into(),
                "Tab" => key = "Tab".into(),
                "Escape" => key = "Escape".into(),
                "Backquote" => key = "grave".into(),
                "Less" => key = "less".into(),
                "Minus" => key = "minus".into(),
                "Equal" => key = "equal".into(),
                "BracketLeft" => key = "bracketleft".into(),
                "BracketRight" => key = "bracketright".into(),
                "Backslash" => key = "backslash".into(),
                "Semicolon" => key = "semicolon".into(),
                "Quote" => key = "apostrophe".into(),
                "Comma" => key = "comma".into(),
                "Period" => key = "period".into(),
                "Slash" => key = "slash".into(),
                "IntlBackslash" => key = "section".into(),
                "F1" | "F2" | "F3" | "F4" | "F5" | "F6" | "F7" | "F8" | "F9" | "F10" | "F11"
                | "F12" => key = p.to_lowercase(),
                s if s.len() == 1 => key = s.to_lowercase(),
                _ => return Err(format!("unsupported key code: {p}")),
            }
        }
        if key.is_empty() {
            return Err("shortcut needs a non-modifier key".into());
        }
        if mods.is_empty() {
            return Err("shortcut needs at least one modifier (Ctrl, Shift, Alt, or Super)".into());
        }
        Ok(format!("{}{}", mods.concat(), key))
    }

    fn count_terminal_profiles_needing_restore() -> Result<u32> {
        let list = gsettings(&["get", "org.gnome.Terminal.ProfilesList", "list"])?;
        let mut count = 0u32;
        for uuid in parse_gsettings_array(&list) {
            let schema = format!(
                "org.gnome.Terminal.Legacy.Keybindings:/org/gnome/terminal/legacy/profiles:/{uuid}/"
            );
            let copy = gsettings(&["get", &schema, "copy"]).unwrap_or_default();
            let paste = gsettings(&["get", &schema, "paste"]).unwrap_or_default();
            let broken_copy = bindings_conflict(&copy, TERMINAL_BROKEN_COPY)
                || bindings_conflict(&copy, "<Control>C");
            let broken_paste = bindings_conflict(&paste, TERMINAL_BROKEN_PASTE)
                || bindings_conflict(&paste, "<Control>V");
            if broken_copy || broken_paste {
                count += 1;
            }
        }
        Ok(count)
    }

    fn desktop_label(desktop: LinuxDesktop) -> &'static str {
        match desktop {
            LinuxDesktop::GnomeWayland => "GNOME (Wayland)",
            LinuxDesktop::CinnamonWayland => "Cinnamon (Wayland)",
            LinuxDesktop::X11 => "X11 (in-app shortcuts)",
            LinuxDesktop::Kde => "KDE Plasma",
            LinuxDesktop::Unknown => "Unknown Wayland",
        }
    }

    fn gnome_family_config(
        desktop: LinuxDesktop,
    ) -> Option<(
        &'static str,
        &'static str,
        &'static str,
        &'static str,
        &'static str,
    )> {
        match desktop {
            LinuxDesktop::GnomeWayland => Some((
                "custom-keybindings",
                GNOME_MEDIA_KEYS,
                GNOME_CUSTOM_SCHEMA,
                "/org/gnome/settings-daemon/plugins/media-keys/custom-keybindings/",
                "inspector-rust-",
            )),
            LinuxDesktop::CinnamonWayland => Some((
                "custom-list",
                CINNAMON_KEYBINDINGS,
                CINNAMON_CUSTOM_SCHEMA,
                "/org/cinnamon/desktop/keybindings/custom-keybindings/",
                "inspector-rust-",
            )),
            _ => None,
        }
    }

    fn read_our_installed_bindings(
        list_schema: &str,
        list_key: &str,
        custom_schema: &str,
        path_prefix: &str,
        id_prefix: &str,
    ) -> Result<Vec<(String, String)>> {
        let mut out = Vec::new();
        let paths = parse_gsettings_array(&gsettings(&["get", list_schema, list_key])?);
        for spec in SHORTCUTS {
            let path = format!("{path_prefix}{id_prefix}{}/", spec.id);
            if !paths.contains(&path) {
                continue;
            }
            let schema = format!("{custom_schema}:{path}");
            let binding = gsettings(&["get", &schema, "binding"]).unwrap_or_default();
            if !binding.is_empty() {
                out.push((spec.id.to_string(), binding));
            }
        }
        Ok(out)
    }

    fn collect_occupied_for_scan(
        list_schema: &str,
        list_key: &str,
        custom_schema: &str,
        id_prefix: &str,
    ) -> Result<HashSet<String>> {
        let mut occupied =
            collect_custom_shortcut_bindings(list_schema, list_key, custom_schema, id_prefix)?;
        occupied.extend(collect_terminal_bindings()?);
        Ok(occupied)
    }

    #[derive(Debug, Clone, Serialize)]
    pub struct ShortcutCandidate {
        pub binding: String,
        pub display: String,
        pub free: bool,
    }

    #[derive(Debug, Clone, Serialize)]
    pub struct ShortcutRow {
        pub id: String,
        pub name: String,
        pub arg: String,
        pub candidates: Vec<ShortcutCandidate>,
        pub chosen: String,
        pub chosen_display: String,
    }

    #[derive(Debug, Clone, Serialize)]
    pub struct ShortcutSetupScan {
        pub desktop: String,
        pub profile: String,
        pub can_configure: bool,
        pub message: Option<String>,
        pub terminal_profiles_to_fix: u32,
        pub rows: Vec<ShortcutRow>,
        pub saved_summary: Option<String>,
    }

    pub fn scan_shortcut_setup(db: &DbHandle) -> Result<ShortcutSetupScan> {
        let desktop = detect();
        let profile = profile_id(desktop).to_string();
        let saved_summary = settings::get(db, SETTINGS_BINDINGS_KEY).ok().flatten();

        let saved_summary_clone = saved_summary.clone();
        let base = || ShortcutSetupScan {
            desktop: desktop_label(desktop).to_string(),
            profile: profile.clone(),
            can_configure: false,
            message: None,
            terminal_profiles_to_fix: 0,
            rows: Vec::new(),
            saved_summary: saved_summary_clone.clone(),
        };

        match desktop {
            LinuxDesktop::X11 => {
                return Ok(ShortcutSetupScan {
                    message: Some(
                        "X11 uses in-app global shortcuts (Ctrl+Shift+V/O/S/C); gsettings setup is not used."
                            .into(),
                    ),
                    ..base()
                });
            }
            LinuxDesktop::Kde => {
                return Ok(ShortcutSetupScan {
                    message: Some(
                        "KDE Plasma: configure shortcuts manually in System Settings → Shortcuts."
                            .into(),
                    ),
                    ..base()
                });
            }
            LinuxDesktop::Unknown => {
                return Ok(ShortcutSetupScan {
                    message: Some(
                        "Unknown desktop: tray menu and CLI flags work; automatic gsettings setup is skipped."
                            .into(),
                    ),
                    ..base()
                });
            }
            _ => {}
        }

        let Some((list_key, list_schema, custom_schema, path_prefix, id_prefix)) =
            gnome_family_config(desktop)
        else {
            return Ok(base());
        };

        let terminal_profiles_to_fix = count_terminal_profiles_needing_restore()?;
        let occupied = collect_occupied_for_scan(list_schema, list_key, custom_schema, id_prefix)?;

        let installed: std::collections::HashMap<String, String> = read_our_installed_bindings(
            list_schema,
            list_key,
            custom_schema,
            path_prefix,
            id_prefix,
        )?
        .into_iter()
        .collect();

        let mut reserved = HashSet::new();
        let mut rows = Vec::new();

        for spec in SHORTCUTS {
            let chosen_from_saved = saved_summary.as_ref().and_then(|s| {
                s.split(',').find_map(|part| {
                    let (id, binding) = part.split_once('=')?;
                    if id == spec.id {
                        Some(binding.to_string())
                    } else {
                        None
                    }
                })
            });
            let installed_binding = installed.get(spec.id).cloned();
            let mut chosen = chosen_from_saved
                .or(installed_binding)
                .or_else(|| pick_binding(spec.binding_candidates, &occupied, &reserved));

            let candidates: Vec<ShortcutCandidate> = spec
                .binding_candidates
                .iter()
                .map(|cand| {
                    let norm = normalize_binding(cand);
                    let free =
                        !norm.is_empty() && !occupied.contains(&norm) && !reserved.contains(&norm);
                    ShortcutCandidate {
                        binding: (*cand).to_string(),
                        display: binding_label(cand),
                        free,
                    }
                })
                .collect();

            if chosen.is_none() {
                chosen = candidates
                    .iter()
                    .find(|c| c.free)
                    .map(|c| c.binding.clone());
            }

            let chosen = chosen.unwrap_or_else(|| spec.binding_candidates[0].to_string());
            let norm = normalize_binding(&chosen);
            if !norm.is_empty() {
                reserved.insert(norm);
            }

            rows.push(ShortcutRow {
                id: spec.id.to_string(),
                name: spec.name.to_string(),
                arg: spec.arg.to_string(),
                candidates,
                chosen_display: binding_label(&chosen),
                chosen: chosen,
            });
        }

        Ok(ShortcutSetupScan {
            can_configure: true,
            terminal_profiles_to_fix,
            rows,
            message: if terminal_profiles_to_fix > 0 {
                Some(format!(
                    "{terminal_profiles_to_fix} GNOME Terminal profile(s) still have broken Ctrl+C/V copy/paste \
                     from an older Inspector build — saving will restore Ctrl+Shift+C/V and move Inspector \
                     shortcuts to free fallbacks."
                ))
            } else {
                None
            },
            ..base()
        })
    }

    fn install_gnome_family_with_bindings(
        db: &DbHandle,
        list_key: &str,
        list_schema: &str,
        custom_schema: &str,
        path_prefix: &str,
        id_prefix: &str,
        resolved: Vec<(&str, String)>,
    ) -> Result<()> {
        let cmd = inspector_command();
        let summary: String = resolved
            .iter()
            .map(|(id, b)| format!("{id}={b}"))
            .collect::<Vec<_>>()
            .join(",");
        settings::set(db, SETTINGS_BINDINGS_KEY, &summary)?;

        let mut paths = parse_gsettings_array(&gsettings(&["get", list_schema, list_key])?);
        remove_our_paths(&mut paths, id_prefix);

        for spec in SHORTCUTS {
            let chosen = resolved
                .iter()
                .find(|(id, _)| *id == spec.id)
                .map(|(_, b)| b.as_str())
                .with_context(|| format!("missing binding for {}", spec.id))?;
            let path = format!("{path_prefix}{id_prefix}{}/", spec.id);
            if !paths.contains(&path) {
                paths.push(path.clone());
            }
            let schema = format!("{custom_schema}:{path}");
            gsettings_set(&["set", &schema, "name", spec.name])?;
            gsettings_set(&["set", &schema, "command", &format!("{cmd} {}", spec.arg)])?;
            gsettings_set(&["set", &schema, "binding", chosen])?;
            tracing::info!("{} → {} ({})", spec.name, binding_label(chosen), chosen);
        }

        gsettings_set(&[
            "set",
            list_schema,
            list_key,
            &format_gsettings_array(&paths),
        ])?;
        Ok(())
    }

    pub fn apply_shortcut_setup(db: &DbHandle, bindings: Vec<(String, String)>) -> Result<()> {
        let desktop = detect();
        let profile = profile_id(desktop);

        let Some((list_key, list_schema, custom_schema, path_prefix, id_prefix)) =
            gnome_family_config(desktop)
        else {
            anyhow::bail!("desktop does not support gsettings shortcut configuration");
        };

        restore_terminal_copy_paste()?;

        let mut occupied =
            collect_custom_shortcut_bindings(list_schema, list_key, custom_schema, id_prefix)?;
        occupied.extend(collect_terminal_bindings()?);

        let mut resolved: Vec<(&str, String)> = Vec::new();
        let mut reserved = HashSet::new();

        for spec in SHORTCUTS {
            let binding_owned = if let Some((_, b)) = bindings.iter().find(|(id, _)| id == spec.id)
            {
                b.clone()
            } else if let Some(picked) = pick_binding(spec.binding_candidates, &occupied, &reserved)
            {
                picked
            } else {
                anyhow::bail!("no binding for {}", spec.name);
            };

            let gsettings_binding = if binding_owned.starts_with('<') {
                binding_owned
            } else {
                web_hotkey_to_gsettings(&binding_owned).map_err(anyhow::Error::msg)?
            };

            let norm = normalize_binding(&gsettings_binding);
            if norm.is_empty() {
                anyhow::bail!("invalid binding for {}", spec.name);
            }
            if occupied.contains(&norm) || reserved.contains(&norm) {
                anyhow::bail!(
                    "{} is already used — pick another combination",
                    binding_label(&gsettings_binding)
                );
            }
            reserved.insert(norm);
            resolved.push((spec.id, gsettings_binding));
        }

        install_gnome_family_with_bindings(
            db,
            list_key,
            list_schema,
            custom_schema,
            path_prefix,
            id_prefix,
            resolved,
        )?;
        settings::set(db, SETTINGS_KEY, profile)?;
        Ok(())
    }

    fn install_gnome_family(
        db: &DbHandle,
        list_key: &str,
        list_schema: &str,
        custom_schema: &str,
        path_prefix: &str,
        id_prefix: &str,
    ) -> Result<()> {
        restore_terminal_copy_paste()?;

        let mut occupied =
            collect_custom_shortcut_bindings(list_schema, list_key, custom_schema, id_prefix)?;
        occupied.extend(collect_terminal_bindings()?);

        let mut reserved = HashSet::new();
        let mut resolved: Vec<(&str, String)> = Vec::new();

        for spec in SHORTCUTS {
            let chosen =
                pick_binding(spec.binding_candidates, &occupied, &reserved).with_context(|| {
                    format!(
                        "no free binding for {} — all candidates occupied",
                        spec.name
                    )
                })?;
            let norm = normalize_binding(&chosen);
            reserved.insert(norm);
            resolved.push((spec.id, chosen));
        }

        install_gnome_family_with_bindings(
            db,
            list_key,
            list_schema,
            custom_schema,
            path_prefix,
            id_prefix,
            resolved,
        )
    }

    pub fn install_gnome(db: &DbHandle) -> Result<()> {
        install_gnome_family(
            db,
            "custom-keybindings",
            GNOME_MEDIA_KEYS,
            GNOME_CUSTOM_SCHEMA,
            "/org/gnome/settings-daemon/plugins/media-keys/custom-keybindings/",
            "inspector-rust-",
        )
    }

    pub fn install_cinnamon(db: &DbHandle) -> Result<()> {
        install_gnome_family(
            db,
            "custom-list",
            CINNAMON_KEYBINDINGS,
            CINNAMON_CUSTOM_SCHEMA,
            "/org/cinnamon/desktop/keybindings/custom-keybindings/",
            "inspector-rust-",
        )
    }

    pub fn profile_id(desktop: LinuxDesktop) -> &'static str {
        match desktop {
            LinuxDesktop::GnomeWayland => "gnome-wayland-v2-auto",
            LinuxDesktop::CinnamonWayland => "cinnamon-wayland-v2-auto",
            LinuxDesktop::X11 => "x11-skip",
            LinuxDesktop::Kde => "kde-manual",
            LinuxDesktop::Unknown => "unknown-skip",
        }
    }

    /// Clear stored profile + re-run conflict scan (for upgrades or `--setup-shortcuts`).
    pub fn force_reinstall(db: &DbHandle) -> Result<()> {
        {
            let conn = db.lock();
            conn.execute(
                "DELETE FROM settings WHERE key IN (?1, ?2)",
                rusqlite::params![SETTINGS_KEY, SETTINGS_BINDINGS_KEY],
            )?;
        }
        try_auto_install(db)
    }

    pub fn try_auto_install(db: &DbHandle) -> Result<()> {
        let desktop = detect();
        let profile = profile_id(desktop);

        if let Ok(existing) = settings::get(db, SETTINGS_KEY) {
            if existing.as_deref() == Some(profile) {
                tracing::info!(
                    "Desktop shortcuts already configured ({profile}); run with cleared setting \
                     `linux.desktop_shortcuts_profile` to re-scan conflicts"
                );
                return Ok(());
            }
        }

        match desktop {
            LinuxDesktop::GnomeWayland => {
                install_gnome(db).context("install GNOME custom shortcuts")?;
                tracing::info!(
                    "Installed GNOME shortcuts after automatic conflict scan (see log for chosen keys)"
                );
            }
            LinuxDesktop::CinnamonWayland => {
                install_cinnamon(db).context("install Cinnamon custom shortcuts")?;
                tracing::info!("Installed Cinnamon shortcuts after automatic conflict scan");
            }
            LinuxDesktop::X11 => {
                tracing::info!("X11 session: using in-app global shortcuts (Ctrl+Shift+V/O/S/C)");
                settings::set(db, SETTINGS_KEY, profile)?;
                return Ok(());
            }
            LinuxDesktop::Kde => {
                tracing::warn!(
                    "KDE Plasma: automatic gsettings shortcuts not available — bind tray/CLI manually"
                );
                return Ok(());
            }
            LinuxDesktop::Unknown => {
                tracing::warn!(
                    "Unknown Linux desktop (Wayland): tray menu and CLI flags work; shortcut auto-setup skipped"
                );
                return Ok(());
            }
        }

        settings::set(db, SETTINGS_KEY, profile)?;
        Ok(())
    }

    const EXPANDER_SHORTCUT_ID: &str = "expander";
    const EXPANDER_SHORTCUT_NAME: &str = "Inspector Rust — Text expander";

    /// True when the text-expander hotkey is registered via gsettings (GNOME/Cinnamon
    /// Wayland), not Tauri's in-app global shortcut.
    pub fn expander_hotkey_needs_gsettings() -> bool {
        !matches!(detect(), LinuxDesktop::X11 | LinuxDesktop::Kde)
    }

    /// Register (or remove) the text-expander hotkey in GNOME/Cinnamon gsettings.
    /// On Wayland, Tauri global shortcuts do not fire — this is the only path that
    /// makes the expander hotkey work without manual GNOME Settings setup.
    pub fn sync_expander_shortcut(_db: &DbHandle, enabled: bool, hotkey: &str) -> Result<()> {
        let desktop = detect();
        let Some((list_key, list_schema, custom_schema, path_prefix, id_prefix)) =
            gnome_family_config(desktop)
        else {
            return Ok(());
        };

        let path = format!("{path_prefix}{id_prefix}{EXPANDER_SHORTCUT_ID}/");
        let mut paths = parse_gsettings_array(&gsettings(&["get", list_schema, list_key])?);

        if !enabled {
            if paths.contains(&path) {
                paths.retain(|p| p != &path);
                gsettings_set(&[
                    "set",
                    list_schema,
                    list_key,
                    &format_gsettings_array(&paths),
                ])?;
                tracing::info!("Removed desktop shortcut for text expander");
            }
            return Ok(());
        }

        let gsettings_binding = web_hotkey_to_gsettings(hotkey).map_err(anyhow::Error::msg)?;
        let norm = normalize_binding(&gsettings_binding);
        if norm.is_empty() {
            anyhow::bail!("invalid expander hotkey");
        }

        let mut occupied =
            collect_custom_shortcut_bindings(list_schema, list_key, custom_schema, id_prefix)?;
        occupied.extend(collect_terminal_bindings()?);

        let schema = format!("{custom_schema}:{path}");
        let current_norm = if paths.contains(&path) {
            gsettings(&["get", &schema, "binding"])
                .ok()
                .map(|b| normalize_binding(&b))
        } else {
            None
        };

        if occupied.contains(&norm) && current_norm.as_ref() != Some(&norm) {
            anyhow::bail!(
                "{} is already used — pick another hotkey (try Alt+1)",
                binding_label(&gsettings_binding)
            );
        }

        let cmd = inspector_command();
        if !paths.contains(&path) {
            paths.push(path.clone());
        }
        gsettings_set(&["set", &schema, "name", EXPANDER_SHORTCUT_NAME])?;
        gsettings_set(&[
            "set",
            &schema,
            "command",
            &format!("{cmd} --expand-at-cursor"),
        ])?;
        gsettings_set(&["set", &schema, "binding", &gsettings_binding])?;
        gsettings_set(&[
            "set",
            list_schema,
            list_key,
            &format_gsettings_array(&paths),
        ])?;
        tracing::info!(
            "{} → {} ({})",
            EXPANDER_SHORTCUT_NAME,
            binding_label(&gsettings_binding),
            gsettings_binding
        );
        Ok(())
    }

    #[cfg(test)]
    mod tests {
        use super::{bindings_conflict, normalize_binding, pick_binding};
        use std::collections::HashSet;

        #[test]
        fn normalize_gnome_accel() {
            assert_eq!(normalize_binding("'<Control><Shift>c'"), "control+shift+c");
            assert_eq!(normalize_binding("<Control>c"), "control+c");
        }

        #[test]
        fn detects_conflict() {
            assert!(bindings_conflict(
                "<Control><Shift>c",
                "'<Control><Shift>c'"
            ));
            assert!(!bindings_conflict("<Control><Shift>c", "<Control>c"));
        }

        #[test]
        fn pick_first_free_candidate() {
            let mut occupied = HashSet::new();
            occupied.insert("control+shift+c".into());
            let reserved = HashSet::new();
            let cands = &["<Control><Shift>c", "<Control><Alt>c"];
            assert_eq!(
                pick_binding(cands, &occupied, &reserved).as_deref(),
                Some("<Control><Alt>c")
            );
        }

        #[test]
        fn web_hotkey_converts_backquote() {
            use super::web_hotkey_to_gsettings;
            assert_eq!(
                web_hotkey_to_gsettings("Ctrl+Backquote").unwrap(),
                "<Control>grave"
            );
            assert_eq!(web_hotkey_to_gsettings("Alt+Digit1").unwrap(), "<Alt>1");
            assert_eq!(
                web_hotkey_to_gsettings("Ctrl+Less").unwrap(),
                "<Control>less"
            );
        }
    }
}

#[cfg(target_os = "linux")]
pub use imp::{
    apply_shortcut_setup, expander_hotkey_needs_gsettings, force_reinstall,
    restore_terminal_and_fix_shortcut_conflicts, scan_shortcut_setup, sync_expander_shortcut,
    try_auto_install, web_hotkey_to_gsettings, ShortcutSetupScan,
};

#[cfg(not(target_os = "linux"))]
#[derive(Debug, Clone, Copy)]
pub enum LinuxDesktop {
    Other,
}

#[cfg(not(target_os = "linux"))]
pub fn detect() -> LinuxDesktop {
    LinuxDesktop::Other
}

#[cfg(not(target_os = "linux"))]
pub fn expander_hotkey_needs_gsettings() -> bool {
    false
}

#[cfg(not(target_os = "linux"))]
pub fn restore_terminal_and_fix_shortcut_conflicts(_db: &DbHandle) -> anyhow::Result<()> {
    Ok(())
}

#[cfg(not(target_os = "linux"))]
pub fn sync_expander_shortcut(_db: &DbHandle, _enabled: bool, _hotkey: &str) -> anyhow::Result<()> {
    Ok(())
}

#[cfg(not(target_os = "linux"))]
pub fn try_auto_install(_db: &DbHandle) -> anyhow::Result<()> {
    Ok(())
}
