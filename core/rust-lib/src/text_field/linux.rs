//! Linux AT-SPI2 implementation of [`FieldAccess`].
//!
//! On GNOME Wayland, enigo/XTest often injects keystrokes into the wrong
//! window (e.g. the XWayland terminal) while the user types in a native
//! Wayland app (WhatsApp, Firefox, gedit). AT-SPI reads the actually focused
//! text field over D-Bus — no synthetic keys, no clipboard roundtrip.

use anyhow::{anyhow, Result};
use atspi::connection::{set_session_accessibility, AccessibilityConnection};
use atspi::proxy::accessible::AccessibleProxy;
use atspi::proxy::proxy_ext::ProxyExt;
use atspi::zbus::proxy::CacheProperties;
use atspi::{Granularity, ObjectRefOwned, Role, State};
use std::sync::atomic::{AtomicBool, Ordering};

use super::{trim_word, FieldAccess, ReplaceOutcome};

const MAX_VISIT_NODES: usize = 800;

static A11Y_ENABLED: AtomicBool = AtomicBool::new(false);

fn ensure_a11y_enabled() {
    if A11Y_ENABLED.swap(true, Ordering::Relaxed) {
        return;
    }
    if let Err(e) = pollster::block_on(set_session_accessibility(true)) {
        tracing::warn!("AT-SPI set_session_accessibility(true): {e}");
    }
}

async fn open_connection() -> Result<AccessibilityConnection> {
    ensure_a11y_enabled();
    AccessibilityConnection::new()
        .await
        .map_err(|e| anyhow!("AT-SPI connection failed: {e}"))
}

async fn proxy_for<'a>(
    conn: &'a AccessibilityConnection,
    obj: &ObjectRefOwned,
) -> Result<AccessibleProxy<'a>> {
    let name = obj
        .name()
        .ok_or_else(|| anyhow!("AT-SPI object missing bus name"))?;
    AccessibleProxy::builder(conn.connection())
        .destination(name.to_owned())?
        .path(obj.path())?
        .cache_properties(CacheProperties::No)
        .build()
        .await
        .map_err(|e| anyhow!("AccessibleProxy build: {e}"))
}

async fn read_word_async() -> Result<Option<String>> {
    let conn = open_connection().await?;
    let root = conn
        .root_accessible_on_registry()
        .await
        .map_err(|e| anyhow!("AT-SPI registry root: {e}"))?;
    let apps = root
        .get_children()
        .await
        .map_err(|e| anyhow!("AT-SPI get_children: {e}"))?;
    let mut stack: Vec<ObjectRefOwned> = apps;
    let mut visited = 0usize;

    while let Some(obj) = stack.pop() {
        if visited >= MAX_VISIT_NODES {
            break;
        }
        visited += 1;
        let node = proxy_for(&conn, &obj).await?;

        if let Ok(states) = node.get_state().await {
            if states.contains(State::Focused) {
                if let Ok(role) = node.get_role().await {
                    if role == Role::PasswordText {
                        continue;
                    }
                }
                if let Ok(proxies) = node.proxies().await {
                    if let Ok(text) = proxies.text().await {
                        let caret = text
                            .caret_offset()
                            .await
                            .map_err(|e| anyhow!("AT-SPI caret_offset: {e}"))?;
                        let gran = Granularity::Word as u32;
                        let (word, _start, _end) =
                            text.get_text_before_offset(caret, gran)
                                .await
                                .map_err(|e| anyhow!("AT-SPI get_text_before_offset: {e}"))?;
                        let trimmed = trim_word(&word);
                        if !trimmed.is_empty() {
                            tracing::info!(
                                "AT-SPI: read word before cursor at {}: {trimmed:?}",
                                node.inner().path()
                            );
                            return Ok(Some(trimmed.to_string()));
                        }
                    }
                }
            }
        }

        if let Ok(children) = node.get_children().await {
            stack.extend(children);
        }
    }

    tracing::debug!("AT-SPI: no focused text field found");
    Ok(None)
}

async fn replace_word_async(replacement: &str) -> Result<ReplaceOutcome> {
    let conn = open_connection().await?;
    let root = conn
        .root_accessible_on_registry()
        .await
        .map_err(|e| anyhow!("AT-SPI registry root: {e}"))?;
    let apps = root.get_children().await.map_err(|e| anyhow!("{e}"))?;
    let mut stack: Vec<ObjectRefOwned> = apps;
    let mut visited = 0usize;

    while let Some(obj) = stack.pop() {
        if visited >= MAX_VISIT_NODES {
            break;
        }
        visited += 1;
        let node = proxy_for(&conn, &obj).await?;

        if let Ok(states) = node.get_state().await {
            if states.contains(State::Focused) {
                if let Ok(role) = node.get_role().await {
                    if role == Role::PasswordText {
                        continue;
                    }
                }
                if let Ok(proxies) = node.proxies().await {
                    if let Ok(text) = proxies.text().await {
                        let caret = text.caret_offset().await?;
                        let gran = Granularity::Word as u32;
                        let (_word, start, _end) = text.get_text_before_offset(caret, gran).await?;
                        if let Ok(ed) = proxies.editable_text().await {
                            let _ = ed.delete_text(start, caret).await;
                            let len = replacement.chars().count() as i32;
                            if ed
                                .insert_text(start, replacement, len)
                                .await
                                .unwrap_or(false)
                            {
                                tracing::info!("AT-SPI: replaced word via EditableText");
                                return Ok(ReplaceOutcome::Replaced);
                            }
                        }
                        let _ = text.set_selection(0, start, caret).await;
                        tracing::info!("AT-SPI: selected abbreviation; caller should paste");
                        return Ok(ReplaceOutcome::SelectionActive);
                    }
                }
            }
        }

        if let Ok(children) = node.get_children().await {
            stack.extend(children);
        }
    }

    Ok(ReplaceOutcome::Unsupported)
}

pub struct AtspiFieldAccess;

impl FieldAccess for AtspiFieldAccess {
    fn read_word_before_cursor(&self) -> Result<Option<String>> {
        pollster::block_on(read_word_async())
    }

    fn try_replace_word_before_cursor(&self, replacement: &str) -> Result<ReplaceOutcome> {
        pollster::block_on(replace_word_async(replacement))
    }
}

/// Whether AT-SPI registry is reachable on the session bus.
pub fn atspi_available() -> bool {
    pollster::block_on(async { open_connection().await.is_ok() })
}
