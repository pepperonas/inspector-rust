//! `bruno` — German income-tax & social-contributions calculator,
//! ported from the maintainer's `steuerschleuder` web app
//! (Brutto-Netto-Rechner 2025, §32a EStG simplified).
//!
//! The compute itself runs in the **frontend** (`core/frontend/src/lib/bruno.ts`)
//! for instant feedback as the user types — no IPC round-trip per
//! keystroke. This module owns only the per-user *defaults*
//! (tax class, state, kids, church, health-insurance Zusatzbeitrag)
//! that get applied when the user types `bruno 60000` without
//! overriding anything. Defaults are persisted in the `settings` table
//! so they survive restarts; the Settings panel has a Bruno section
//! for editing them.

use anyhow::Result;
use serde::{Deserialize, Serialize};

use crate::db::DbHandle;
use crate::settings;

/// Settings-table key prefix. Each field is stored as a separate
/// `bruno.<field>` row so partial updates don't need a JSON
/// round-trip + nothing else's serialisation choice leaks here.
const KEY_TAX_CLASS: &str = "bruno.tax_class";
const KEY_STATE: &str = "bruno.state";
const KEY_CHILDREN: &str = "bruno.children";
const KEY_CHURCH: &str = "bruno.church_member";
const KEY_HEALTH_ADD: &str = "bruno.health_add";

/// Per-user defaults applied to a bare `bruno <€>` invocation. Picked
/// to match the most common German worker: single, child-free, NRW
/// (largest state), Techniker-Krankenkasse Zusatzbeitrag 2025
/// (`2.45%`, slightly rounded). All overridable in Settings.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BrunoDefaults {
    /// Lohnsteuerklasse 1..=6. Class 1 = single / no kids.
    pub tax_class: u8,
    /// Bundesland short ISO code (`bw`, `by`, `be`, …, `th`). Drives
    /// Kirchensteuersatz when `church_member=true` (BW + BY: 8 %, else
    /// 9 %).
    pub state: String,
    /// Number of children — drives Pflegeversicherung discount
    /// (`§ 55 SGB XI`, -0.25 % per kid #2..=5, capped at -2 %) and
    /// the Kinderfreibetrag.
    pub children: u32,
    /// `true` → Kirchensteuer applies.
    pub is_church_member: bool,
    /// Krankenkasse-Zusatzbeitrag in **percent** (e.g. `2.45` = 2.45 %).
    /// Avg. statutory 2025 is ~2.5 %; TK specifically is 2.45 %.
    pub health_add: f64,
}

impl Default for BrunoDefaults {
    fn default() -> Self {
        Self {
            tax_class: 1,
            state: "nw".to_string(),
            children: 0,
            is_church_member: false,
            health_add: 2.45,
        }
    }
}

pub fn get_defaults(db: &DbHandle) -> Result<BrunoDefaults> {
    let def = BrunoDefaults::default();
    Ok(BrunoDefaults {
        tax_class: settings::get_or(db, KEY_TAX_CLASS, &def.tax_class.to_string())
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(def.tax_class),
        state: settings::get_or(db, KEY_STATE, &def.state)
            .unwrap_or(def.state.clone()),
        children: settings::get_or(db, KEY_CHILDREN, &def.children.to_string())
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(def.children),
        is_church_member: settings::get_bool(db, KEY_CHURCH, def.is_church_member)
            .unwrap_or(def.is_church_member),
        health_add: settings::get_or(db, KEY_HEALTH_ADD, &def.health_add.to_string())
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(def.health_add),
    })
}

pub fn set_defaults(db: &DbHandle, defs: &BrunoDefaults) -> Result<()> {
    let tax_class = defs.tax_class.clamp(1, 6);
    // States we recognise — defensive against typos / injected garbage
    // hitting churchTax(). Anything unknown falls back to the default
    // 9 % rate downstream, but persisting only the whitelist keeps
    // the Settings UI honest.
    const STATES: &[&str] = &[
        "bw", "by", "be", "bb", "hb", "hh", "he", "mv", "ni", "nw", "rp", "sl", "sn",
        "st", "sh", "th",
    ];
    let state = if STATES.contains(&defs.state.as_str()) {
        defs.state.clone()
    } else {
        "nw".to_string()
    };
    let health_add = defs.health_add.clamp(0.0, 10.0);
    let children = defs.children.min(20);

    settings::set(db, KEY_TAX_CLASS, &tax_class.to_string())?;
    settings::set(db, KEY_STATE, &state)?;
    settings::set(db, KEY_CHILDREN, &children.to_string())?;
    settings::set(db, KEY_CHURCH, if defs.is_church_member { "true" } else { "false" })?;
    settings::set(db, KEY_HEALTH_ADD, &health_add.to_string())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;

    fn mem_db() -> DbHandle {
        let conn = Connection::open_in_memory().unwrap();
        let h = std::sync::Arc::new(parking_lot::Mutex::new(conn));
        settings::init_table(&h).unwrap();
        h
    }

    #[test]
    fn defaults_round_trip() {
        let db = mem_db();
        let custom = BrunoDefaults {
            tax_class: 3,
            state: "by".to_string(),
            children: 2,
            is_church_member: true,
            health_add: 2.7,
        };
        set_defaults(&db, &custom).unwrap();
        let back = get_defaults(&db).unwrap();
        assert_eq!(back.tax_class, 3);
        assert_eq!(back.state, "by");
        assert_eq!(back.children, 2);
        assert!(back.is_church_member);
        assert!((back.health_add - 2.7).abs() < 1e-6);
    }

    #[test]
    fn defaults_when_no_settings_use_fallback() {
        let db = mem_db();
        let back = get_defaults(&db).unwrap();
        assert_eq!(back.tax_class, 1);
        assert_eq!(back.state, "nw");
        assert_eq!(back.children, 0);
        assert!(!back.is_church_member);
        assert!((back.health_add - 2.45).abs() < 1e-6);
    }

    #[test]
    fn unknown_state_is_coerced_to_nw() {
        let db = mem_db();
        let bad = BrunoDefaults {
            state: "xx".to_string(),
            ..Default::default()
        };
        set_defaults(&db, &bad).unwrap();
        assert_eq!(get_defaults(&db).unwrap().state, "nw");
    }

    #[test]
    fn tax_class_clamped_to_1_6() {
        let db = mem_db();
        let bad = BrunoDefaults {
            tax_class: 99,
            ..Default::default()
        };
        set_defaults(&db, &bad).unwrap();
        assert_eq!(get_defaults(&db).unwrap().tax_class, 6);
    }
}
