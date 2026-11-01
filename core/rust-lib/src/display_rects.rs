//! CoreGraphics display rects (macOS).

// Physical display rects in active-display order (main display first), matching
// the order ffmpeg's avfoundation uses for its `Capture screen N` devices.
// Same FFI pattern as `brightness.rs`.

type CGDirectDisplayID = u32;

#[repr(C)]
struct CGPoint {
    x: f64,
    y: f64,
}
#[repr(C)]
struct CGSize {
    width: f64,
    height: f64,
}
#[repr(C)]
struct CGRect {
    origin: CGPoint,
    size: CGSize,
}

#[link(name = "CoreGraphics", kind = "framework")]
extern "C" {
    fn CGGetActiveDisplayList(
        max_displays: u32,
        active: *mut CGDirectDisplayID,
        display_count: *mut u32,
    ) -> i32;
    fn CGDisplayBounds(display: CGDirectDisplayID) -> CGRect;
    fn CGDisplayPixelsWide(display: CGDirectDisplayID) -> usize;
    fn CGDisplayPixelsHigh(display: CGDirectDisplayID) -> usize;
}

/// `(x, y, w, h)` per display in **physical pixels**, active-display order.
/// `CGDisplayBounds` is in points; we scale by each display's pixels/points
/// ratio. (Assumes a uniform scale factor across displays — the common case;
/// mixed-DPI layouts may be slightly off.) Empty on failure → caller falls
/// back to the primary screen.
pub fn physical_rects() -> Vec<(i32, i32, u32, u32)> {
    let mut ids = [0u32; 16];
    let mut count = 0u32;
    let err = unsafe { CGGetActiveDisplayList(16, ids.as_mut_ptr(), &mut count) };
    if err != 0 {
        return Vec::new();
    }
    let mut out = Vec::new();
    for &id in ids.iter().take(count as usize) {
        let b = unsafe { CGDisplayBounds(id) };
        let pw = unsafe { CGDisplayPixelsWide(id) } as f64;
        let ph = unsafe { CGDisplayPixelsHigh(id) } as f64;
        let scale = if b.size.width > 0.0 { pw / b.size.width } else { 1.0 };
        out.push((
            (b.origin.x * scale).round() as i32,
            (b.origin.y * scale).round() as i32,
            pw as u32,
            ph as u32,
        ));
    }
    out
}
