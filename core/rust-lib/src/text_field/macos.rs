//! macOS Accessibility (AXUIElement) implementation of `FieldAccess`.
//!
//! References:
//! - <https://developer.apple.com/documentation/applicationservices/axuielement_h>
//! - The "Accessibility Programming Guidelines for OS X"
//!
//! Permission requirement: the same `AXIsProcessTrusted()` permission
//! we already require for keystroke synthesis. Granting "Accessibility"
//! to Inspector Rust in System Settings unlocks both paths.

use anyhow::{anyhow, Result};
use std::ffi::{c_void, CString};

use super::{trim_word, word_start_before_cursor, FieldAccess, ReplaceOutcome};

// ── Core Foundation FFI ─────────────────────────────────────────────────────

type CFTypeRef = *const c_void;
type CFAllocatorRef = *const c_void;
type CFStringRef = *const c_void;
type CFRange = (CFIndex, CFIndex);
type CFIndex = isize;
type Boolean = u8;

const KCF_STRING_ENCODING_UTF8: u32 = 0x08000100;

type CFArrayRef = *const c_void;

#[link(name = "CoreFoundation", kind = "framework")]
extern "C" {
    static kCFAllocatorDefault: CFAllocatorRef;
    static kCFTypeArrayCallBacks: c_void;

    fn CFStringCreateWithCString(
        allocator: CFAllocatorRef,
        c_str: *const i8,
        encoding: u32,
    ) -> CFStringRef;

    fn CFStringGetLength(s: CFStringRef) -> CFIndex;

    fn CFStringGetCString(
        s: CFStringRef,
        buffer: *mut i8,
        buffer_size: CFIndex,
        encoding: u32,
    ) -> Boolean;

    fn CFStringGetMaximumSizeForEncoding(length: CFIndex, encoding: u32) -> CFIndex;

    fn CFArrayCreate(
        allocator: CFAllocatorRef,
        values: *const CFTypeRef,
        num_values: CFIndex,
        callbacks: *const c_void,
    ) -> CFArrayRef;

    fn CFArrayGetCount(array: CFArrayRef) -> CFIndex;
    fn CFArrayGetValueAtIndex(array: CFArrayRef, idx: CFIndex) -> CFTypeRef;

    fn CFRelease(cf: CFTypeRef);
    fn CFGetTypeID(cf: CFTypeRef) -> usize;
    fn CFStringGetTypeID() -> usize;
}

// ── ApplicationServices / AXUIElement FFI ───────────────────────────────────

type AXUIElementRef = *const c_void;
type AXValueRef = *const c_void;
type AXError = i32;

const KAX_ERROR_SUCCESS: AXError = 0;

// AXValue type IDs (from AXValue.h).
const KAX_VALUE_CFRANGE_TYPE: u32 = 4;

#[link(name = "ApplicationServices", kind = "framework")]
extern "C" {
    fn AXUIElementCreateSystemWide() -> AXUIElementRef;

    fn AXUIElementCopyAttributeValue(
        element: AXUIElementRef,
        attribute: CFStringRef,
        out_value: *mut CFTypeRef,
    ) -> AXError;

    /// Fetch multiple attributes in **one** kernel round-trip. Each
    /// returned element is either the attribute value or a CFNull
    /// (we can't tell missing vs. null from the API surface).
    fn AXUIElementCopyMultipleAttributeValues(
        element: AXUIElementRef,
        attributes: CFArrayRef,
        options: u32,
        out_values: *mut CFArrayRef,
    ) -> AXError;

    fn AXUIElementSetAttributeValue(
        element: AXUIElementRef,
        attribute: CFStringRef,
        value: CFTypeRef,
    ) -> AXError;

    fn AXValueCreate(value_type: u32, value_ptr: *const c_void) -> AXValueRef;
    fn AXValueGetValue(value: AXValueRef, value_type: u32, value_ptr: *mut c_void) -> Boolean;
    fn AXValueGetType(value: AXValueRef) -> u32;
}

// ── AX attribute name constants (constructed at runtime as CFStrings) ──────

fn cf_string(s: &str) -> CFStringRef {
    let c = CString::new(s).expect("AX attribute name must be ASCII");
    unsafe { CFStringCreateWithCString(kCFAllocatorDefault, c.as_ptr(), KCF_STRING_ENCODING_UTF8) }
}

/// Cached, deliberately-leaked `CFStringRef` for an AX attribute name.
/// Avoids re-creating + re-releasing the same ~10 strings per
/// expansion. CF strings are reference-counted; one leaked ref per
/// process is harmless (the strings live for the process lifetime
/// anyway since they're const at the call sites).
mod attr {
    use super::{cf_string, CFStringRef};
    use std::sync::OnceLock;

    macro_rules! cached {
        ($name:ident, $value:literal) => {
            pub fn $name() -> CFStringRef {
                // We can't store *const c_void in OnceLock directly
                // (not Send/Sync); wrap in a usize-cast helper.
                static CACHE: OnceLock<usize> = OnceLock::new();
                *CACHE.get_or_init(|| cf_string($value) as usize) as CFStringRef
            }
        };
    }

    cached!(focused_ui_element, "AXFocusedUIElement");
    cached!(value, "AXValue");
    cached!(selected_text_range, "AXSelectedTextRange");
    cached!(selected_text, "AXSelectedText");
    cached!(subrole, "AXSubrole");
}

/// Convert a `CFStringRef` to an owned Rust `String`. Returns `None` if
/// the CF object isn't actually a CFString or if conversion fails.
unsafe fn cf_string_to_rust(cf: CFTypeRef) -> Option<String> {
    if cf.is_null() {
        return None;
    }
    if CFGetTypeID(cf) != CFStringGetTypeID() {
        return None;
    }
    let len = CFStringGetLength(cf);
    let max = CFStringGetMaximumSizeForEncoding(len, KCF_STRING_ENCODING_UTF8) + 1;
    let mut buf = vec![0i8; max as usize];
    let ok = CFStringGetCString(cf, buf.as_mut_ptr(), max, KCF_STRING_ENCODING_UTF8);
    if ok == 0 {
        return None;
    }
    let bytes: Vec<u8> = buf
        .into_iter()
        .take_while(|&b| b != 0)
        .map(|b| b as u8)
        .collect();
    String::from_utf8(bytes).ok()
}

// ── Public impl ─────────────────────────────────────────────────────────────

pub struct AxFieldAccess;

impl AxFieldAccess {
    /// One-shot: read focused element, return (`value`, `cursor_chars`)
    /// where `cursor_chars` is the cursor's start position in UTF-16
    /// code units, or `None` if the element doesn't expose the
    /// necessary attributes.
    ///
    /// **v0.35.0 optimisation**: batches `AXValue` + `AXSelectedTextRange`
    /// reads into a single `AXUIElementCopyMultipleAttributeValues`
    /// call instead of two separate `AXUIElementCopyAttributeValue`
    /// round-trips. Each AX round-trip is ~2–5 ms (XPC IPC to the
    /// accessibility daemon), so one batched call vs. two sequential
    /// ones saves ~5 ms per expansion.
    fn read_focused(&self) -> Result<Option<(AXUIElementRef, String, usize)>> {
        unsafe {
            let system = AXUIElementCreateSystemWide();
            if system.is_null() {
                return Err(anyhow!("AXUIElementCreateSystemWide returned null"));
            }

            // 1) Find the focused UI element. (Can't batch with the
            //    subsequent reads — those go to *this* element, not
            //    the system-wide one.)
            let mut focused_value: CFTypeRef = std::ptr::null();
            let err = AXUIElementCopyAttributeValue(
                system,
                attr::focused_ui_element(),
                &mut focused_value,
            );
            CFRelease(system);
            if err != KAX_ERROR_SUCCESS || focused_value.is_null() {
                return Ok(None);
            }
            let focused: AXUIElementRef = focused_value;

            // 2) Batch-read AXValue + AXSelectedTextRange in one call.
            let attrs: [CFTypeRef; 2] = [
                attr::value() as CFTypeRef,
                attr::selected_text_range() as CFTypeRef,
            ];
            let attrs_array = CFArrayCreate(
                kCFAllocatorDefault,
                attrs.as_ptr(),
                2,
                &kCFTypeArrayCallBacks as *const _ as *const c_void,
            );
            if attrs_array.is_null() {
                CFRelease(focused);
                return Ok(None);
            }
            let mut values_array: CFArrayRef = std::ptr::null();
            let err = AXUIElementCopyMultipleAttributeValues(
                focused,
                attrs_array,
                0, // no kAXCopyMultipleAttributeOptionStopOnError
                &mut values_array,
            );
            CFRelease(attrs_array);
            if err != KAX_ERROR_SUCCESS || values_array.is_null() {
                CFRelease(focused);
                return Ok(None);
            }
            // Returned array always has the same length as the request
            // (positions hold CFNull for missing attributes — we treat
            // those as Unsupported).
            if CFArrayGetCount(values_array) < 2 {
                CFRelease(values_array);
                CFRelease(focused);
                return Ok(None);
            }
            // The array owns its contents; CFArrayGetValueAtIndex
            // returns a borrowed CFTypeRef (no retain needed for our
            // immediate read).
            let value_cf = CFArrayGetValueAtIndex(values_array, 0);
            let range_cf = CFArrayGetValueAtIndex(values_array, 1);

            // 2a) Decode AXValue.
            let Some(value_str) = cf_string_to_rust(value_cf) else {
                CFRelease(values_array);
                CFRelease(focused);
                return Ok(None);
            };

            // 2b) Decode AXSelectedTextRange — must be a CFRange-bearing
            //     AXValue.
            if range_cf.is_null() || AXValueGetType(range_cf) != KAX_VALUE_CFRANGE_TYPE {
                CFRelease(values_array);
                CFRelease(focused);
                return Ok(None);
            }
            let mut range: CFRange = (0, 0);
            let got = AXValueGetValue(
                range_cf,
                KAX_VALUE_CFRANGE_TYPE,
                &mut range as *mut _ as *mut c_void,
            );
            CFRelease(values_array);
            if got == 0 {
                CFRelease(focused);
                return Ok(None);
            }
            let cursor_utf16 = range.0.max(0) as usize;
            let cursor_chars = utf16_units_to_char_index(&value_str, cursor_utf16);

            Ok(Some((focused, value_str, cursor_chars)))
        }
    }
}

impl FieldAccess for AxFieldAccess {
    fn is_focused_field_secure(&self) -> Result<bool> {
        unsafe {
            let system = AXUIElementCreateSystemWide();
            if system.is_null() {
                return Ok(false);
            }
            let mut focused_value: CFTypeRef = std::ptr::null();
            let err = AXUIElementCopyAttributeValue(
                system,
                attr::focused_ui_element(),
                &mut focused_value,
            );
            CFRelease(system);
            if err != KAX_ERROR_SUCCESS || focused_value.is_null() {
                return Ok(false);
            }
            let focused: AXUIElementRef = focused_value;

            // Subrole is the standard way Cocoa text fields signal
            // their security flag. NSSecureTextField sets
            // `AXSubrole == "AXSecureTextField"`. WKWebView'd password
            // inputs are a different story (they go through Web AX
            // and expose `AXRole == "AXTextField"` + sometimes a
            // `AXSubrole` of `AXSecureTextField` when input type
            // is "password"). Checking the subrole catches both.
            let mut subrole_cf: CFTypeRef = std::ptr::null();
            let err = AXUIElementCopyAttributeValue(focused, attr::subrole(), &mut subrole_cf);
            CFRelease(focused);
            if err != KAX_ERROR_SUCCESS || subrole_cf.is_null() {
                return Ok(false);
            }
            let subrole = cf_string_to_rust(subrole_cf);
            CFRelease(subrole_cf);
            Ok(matches!(subrole.as_deref(), Some("AXSecureTextField")))
        }
    }

    fn read_word_before_cursor(&self) -> Result<Option<String>> {
        let Some((focused, value, cursor_chars)) = self.read_focused()? else {
            return Ok(None);
        };
        unsafe { CFRelease(focused) };
        let start = word_start_before_cursor(&value, cursor_chars);
        // Compute the byte slice [start..cursor-byte] of `value`.
        let cursor_byte: usize = value
            .char_indices()
            .nth(cursor_chars)
            .map(|(i, _)| i)
            .unwrap_or(value.len());
        if start >= cursor_byte {
            return Ok(None);
        }
        let word = trim_word(&value[start..cursor_byte]).to_string();
        if word.is_empty() {
            return Ok(None);
        }
        Ok(Some(word))
    }

    fn try_replace_word_before_cursor(&self, replacement: &str) -> Result<ReplaceOutcome> {
        let Some((focused, value, cursor_chars)) = self.read_focused()? else {
            return Ok(ReplaceOutcome::Unsupported);
        };
        let start_byte = word_start_before_cursor(&value, cursor_chars);
        // The cursor in *char* index → byte index.
        let cursor_byte: usize = value
            .char_indices()
            .nth(cursor_chars)
            .map(|(i, _)| i)
            .unwrap_or(value.len());
        if start_byte >= cursor_byte {
            // Nothing before the cursor — nothing to replace.
            unsafe { CFRelease(focused) };
            return Ok(ReplaceOutcome::Unsupported);
        }

        // Compute the word range as UTF-16 code units (which is what AX
        // expects when we set the selected range).
        let prefix = &value[..start_byte];
        let word = &value[start_byte..cursor_byte];
        let start_utf16: isize = utf16_count(prefix) as isize;
        let length_utf16: isize = utf16_count(word) as isize;

        unsafe {
            // 1) Set kAXSelectedTextRangeAttribute to (start, length) of
            //    the word — this *selects* the abbreviation. If even this
            //    fails the element exposes no settable text attributes;
            //    let the caller do the full keystroke fallback.
            let range = (start_utf16, length_utf16);
            let range_value =
                AXValueCreate(KAX_VALUE_CFRANGE_TYPE, &range as *const _ as *const c_void);
            if range_value.is_null() {
                CFRelease(focused);
                return Ok(ReplaceOutcome::Unsupported);
            }
            let err =
                AXUIElementSetAttributeValue(focused, attr::selected_text_range(), range_value);
            CFRelease(range_value);
            if err != KAX_ERROR_SUCCESS {
                CFRelease(focused);
                return Ok(ReplaceOutcome::Unsupported);
            }

            // 2) Set kAXSelectedTextAttribute to the replacement string.
            //    On a well-behaved Cocoa text view this replaces the
            //    selected range in place. On Electron / Chromium /
            //    Mac-Catalyst text views it commonly returns
            //    kAXErrorSuccess but does nothing — so we don't trust the
            //    return code; we verify by re-reading AXValue below.
            let replacement_cf = {
                let c = CString::new(replacement.replace('\0', "")).unwrap();
                CFStringCreateWithCString(kCFAllocatorDefault, c.as_ptr(), KCF_STRING_ENCODING_UTF8)
            };
            if replacement_cf.is_null() {
                CFRelease(focused);
                // The range was set in step 1 → the abbreviation is
                // selected → caller can paste over it.
                return Ok(ReplaceOutcome::SelectionActive);
            }
            let _ = AXUIElementSetAttributeValue(focused, attr::selected_text(), replacement_cf);
            CFRelease(replacement_cf);

            // 3) Verify. Poll AXValue up to 60 ms (12 × 5 ms). The old
            //    single 15 ms sleep was fragile under load — slow
            //    Electron apps occasionally took 20-40 ms to apply
            //    the AX set, and we'd mis-classify those as
            //    SelectionActive and then double-paste. Poll wins on
            //    both: returns *fast* (5-10 ms) when the app is
            //    snappy, gives the slow ones a fair shake.
            let mut new_value: Option<String> = None;
            for _attempt in 0..12 {
                std::thread::sleep(std::time::Duration::from_millis(5));
                let mut new_value_cf: CFTypeRef = std::ptr::null();
                let verr = AXUIElementCopyAttributeValue(focused, attr::value(), &mut new_value_cf);
                if verr == KAX_ERROR_SUCCESS && !new_value_cf.is_null() {
                    let s = cf_string_to_rust(new_value_cf);
                    CFRelease(new_value_cf);
                    if let Some(nv) = s {
                        if nv != value {
                            new_value = Some(nv);
                            break;
                        }
                        // Same as before — keep polling.
                    }
                } else if !new_value_cf.is_null() {
                    CFRelease(new_value_cf);
                }
            }
            CFRelease(focused);

            match new_value {
                Some(_) => Ok(ReplaceOutcome::Replaced),
                None => Ok(ReplaceOutcome::SelectionActive),
            }
        }
    }
}

// ── UTF-16 helpers ──────────────────────────────────────────────────────────

fn utf16_count(s: &str) -> usize {
    s.chars().map(|c| c.len_utf16()).sum()
}

/// Convert a UTF-16 code-unit offset into a Rust char index.
fn utf16_units_to_char_index(s: &str, target_units: usize) -> usize {
    let mut units = 0;
    for (char_idx, c) in s.chars().enumerate() {
        if units >= target_units {
            return char_idx;
        }
        units += c.len_utf16();
    }
    s.chars().count()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn utf16_units_ascii_match_chars() {
        assert_eq!(utf16_units_to_char_index("hello", 3), 3);
        assert_eq!(utf16_count("hello"), 5);
    }

    #[test]
    fn utf16_units_count_supplementary_as_two() {
        // 🚀 is U+1F680 — outside BMP, takes 2 UTF-16 units.
        let s = "🚀abc";
        // Cursor in UTF-16 units = 2 → after the rocket → char index 1.
        assert_eq!(utf16_units_to_char_index(s, 2), 1);
        // Cursor in UTF-16 units = 4 → after "🚀ab" → char index 3.
        assert_eq!(utf16_units_to_char_index(s, 4), 3);
        assert_eq!(utf16_count("🚀abc"), 5); // 2 + 3
    }

    #[test]
    fn utf16_units_handle_german_umlauts_as_single_unit() {
        // Größe — ö is BMP, single UTF-16 unit.
        assert_eq!(utf16_count("Größe"), 5);
        assert_eq!(utf16_units_to_char_index("Größe", 5), 5);
    }
}
