//! Source selection through Apple's `SCContentSharingPicker` (macOS 14+).
//!
//! The picker is system UI shown on the main thread; its answer arrives on
//! an observer (`IRRecordPickerObserver`) and is kept here until the next
//! pick. A filter is an Objective-C object that cannot be serialised, so the
//! session takes it from [`current`] at start time.
//!
//! Like OpenScreen's `PickerSession.swift`: `allowsChangingSelectedContent =
//! false` (a new source is a new pick), the picker is switched off and on
//! before every presentation (an active picker can replay an old pick as a
//! new answer), and a selection that arrives while no presentation of ours
//! waits for one is ignored.

use std::cell::RefCell;
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};

use objc2::rc::Retained;
use objc2::runtime::{AnyClass, AnyObject, NSObject, NSObjectProtocol, ProtocolObject};
use objc2::{define_class, msg_send, sel, AllocAnyThread, DefinedClass, Message};
use objc2_core_foundation::CGRect;
use objc2_foundation::{NSArray, NSError, NSString};
use objc2_screen_capture_kit::{
    SCContentFilter, SCContentSharingPicker, SCContentSharingPickerConfiguration, SCContentSharingPickerMode,
    SCContentSharingPickerObserver, SCShareableContentStyle, SCStream,
};
use parking_lot::Mutex;
use serde::Serialize;
use tauri::{AppHandle, Emitter};

use super::capture_macos;
use super::project::SourceKind;

/// `SCContentSharingPicker` exists (macOS 14+) — checked at run time so an
/// older system degrades to "unsupported" instead of crashing.
pub fn picker_available() -> bool {
    AnyClass::get(c"SCContentSharingPicker").is_some()
}

struct Sendable<T>(T);
// SAFETY: SCContentFilter is immutable after creation; ScreenCaptureKit
// accepts it on any thread.
unsafe impl<T> Send for Sendable<T> {}
unsafe impl<T> Sync for Sendable<T> {}

/// What the user picked.
pub struct Picked {
    pub kind: SourceKind,
    pub label: String,
    pub window_id: Option<u32>,
    pub display_id: Option<u32>,
    pub content_w_pt: f64,
    pub content_h_pt: f64,
    pub scale: f64,
    filter: Sendable<Retained<SCContentFilter>>,
    /// The picker config listed our own bundle id (excludes the whole app).
    excludes_app: bool,
}

impl Clone for Picked {
    fn clone(&self) -> Self {
        Self {
            kind: self.kind,
            label: self.label.clone(),
            window_id: self.window_id,
            display_id: self.display_id,
            content_w_pt: self.content_w_pt,
            content_h_pt: self.content_h_pt,
            scale: self.scale,
            filter: Sendable(self.filter.0.clone()),
            excludes_app: self.excludes_app,
        }
    }
}

impl Picked {
    fn from_filter(filter: Retained<SCContentFilter>, kind: SourceKind, excludes_app: bool) -> Self {
        let (rect, scale) = unsafe { (filter.contentRect(), filter.pointPixelScale() as f64) };
        let mut p = Picked {
            kind,
            label: String::new(),
            window_id: None,
            display_id: None,
            content_w_pt: rect.size.width,
            content_h_pt: rect.size.height,
            scale,
            filter: Sendable(filter),
            excludes_app,
        };
        p.inspect(rect);
        p
    }

    /// Fill kind/ids/label from `includedWindows`/`includedDisplays`
    /// (macOS 15.2+; older systems keep the requested kind).
    fn inspect(&mut self, rect: CGRect) {
        let f = &self.filter.0;
        let fobj: &AnyObject = f.as_ref();
        unsafe {
            if fobj.class().responds_to(sel!(includedWindows)) {
                let wins: Retained<NSArray<AnyObject>> = msg_send![&**f, includedWindows];
                if wins.count() == 1 {
                    if let Some(w) = wins.firstObject() {
                        self.kind = SourceKind::Window;
                        let id: u32 = msg_send![&*w, windowID];
                        self.window_id = Some(id);
                        let title: Option<Retained<NSString>> = msg_send![&*w, title];
                        let app: Option<Retained<AnyObject>> = msg_send![&*w, owningApplication];
                        let app_name: Option<Retained<NSString>> = match &app {
                            Some(a) => msg_send![&**a, applicationName],
                            None => None,
                        };
                        let title = title.map(|s| s.to_string()).unwrap_or_default();
                        let app_name = app_name.map(|s| s.to_string()).unwrap_or_default();
                        self.label = match (app_name.is_empty(), title.is_empty()) {
                            (false, false) => format!("{app_name} — {title}"),
                            (false, true) => app_name,
                            (true, false) => title,
                            (true, true) => crate::i18n::t("rec2.source_window"),
                        };
                    }
                }
            }
            if self.kind == SourceKind::Display && fobj.class().responds_to(sel!(includedDisplays)) {
                let ds: Retained<NSArray<AnyObject>> = msg_send![&**f, includedDisplays];
                if let Some(d) = ds.firstObject() {
                    let id: u32 = msg_send![&*d, displayID];
                    self.display_id = Some(id);
                }
            }
        }
        if self.label.is_empty() {
            self.label = match self.kind {
                SourceKind::Display => crate::i18n::tf(
                    "rec2.source_display",
                    &[("size", &format!("{}×{}", rect.size.width.round(), rect.size.height.round()))],
                ),
                SourceKind::Window => crate::i18n::t("rec2.source_window"),
            };
        }
    }

    /// For tests: a whole display, our app excluded.
    #[cfg(test)]
    pub fn display(display_id: u32) -> Result<Picked, String> {
        let f = capture_macos::display_filter(display_id)?;
        let mut p = Picked::from_filter(f, SourceKind::Display, true);
        p.display_id = Some(display_id);
        Ok(p)
    }

    /// The filter to record with. A picked display is rebuilt as our own
    /// display filter (excludes every window of this app, also later ones)
    /// when Screen Recording is granted — the picker's filter otherwise.
    pub fn filter_for_capture(&self) -> Retained<SCContentFilter> {
        if self.kind == SourceKind::Display
            && !self.excludes_app
            && crate::screen_recording::screen_recording_granted()
        {
            let id = self.display_id.unwrap_or_else(capture_macos::main_display_id);
            match capture_macos::display_filter(id) {
                Ok(f) => return f,
                Err(e) => tracing::warn!("record: own display filter failed ({e}); using the picker's"),
            }
        }
        self.filter.0.clone()
    }
}

static PICKED: Mutex<Option<Picked>> = Mutex::new(None);
static AWAITING: AtomicBool = AtomicBool::new(false);
static EXCLUDES_APP: AtomicBool = AtomicBool::new(false);
/// 0 = display, 1 = window (what the last presentation asked for).
static REQUESTED: AtomicU8 = AtomicU8::new(0);

/// The current pick, if any.
pub fn current() -> Option<Picked> {
    PICKED.lock().clone()
}

#[derive(Serialize, Clone)]
struct PickedEvent {
    label: String,
    kind: SourceKind,
}

#[derive(Serialize, Clone)]
struct CancelledEvent {
    error: Option<String>,
}

pub struct Ivars {
    app: AppHandle,
}

define_class!(
    #[unsafe(super(NSObject))]
    #[name = "IRRecordPickerObserver"]
    #[ivars = Ivars]
    struct PickerObserver;

    unsafe impl NSObjectProtocol for PickerObserver {}

    unsafe impl SCContentSharingPickerObserver for PickerObserver {
        #[unsafe(method(contentSharingPicker:didCancelForStream:))]
        fn did_cancel(&self, _picker: &SCContentSharingPicker, stream: Option<&SCStream>) {
            if stream.is_none() && AWAITING.swap(false, Ordering::SeqCst) {
                let app = &self.ivars().app;
                let _ = app.emit("record-pick-cancelled", CancelledEvent { error: None });
                super::session::pick_finished(app);
            }
        }

        #[unsafe(method(contentSharingPicker:didUpdateWithFilter:forStream:))]
        fn did_update(&self, _picker: &SCContentSharingPicker, filter: &SCContentFilter, stream: Option<&SCStream>) {
            if stream.is_some() {
                return;
            }
            if !AWAITING.swap(false, Ordering::SeqCst) {
                tracing::info!("record: ignored a picker selection nobody asked for");
                return;
            }
            let kind = if REQUESTED.load(Ordering::SeqCst) == 1 { SourceKind::Window } else { SourceKind::Display };
            let picked = Picked::from_filter(filter.retain(), kind, EXCLUDES_APP.load(Ordering::SeqCst));
            let ev = PickedEvent { label: picked.label.clone(), kind: picked.kind };
            *PICKED.lock() = Some(picked);
            let app = &self.ivars().app;
            let _ = app.emit("record-source-picked", ev);
            super::session::pick_finished(app);
        }

        #[unsafe(method(contentSharingPickerStartDidFailWithError:))]
        fn did_fail(&self, error: &NSError) {
            AWAITING.store(false, Ordering::SeqCst);
            let msg = error.localizedDescription().to_string();
            tracing::warn!("record: picker failed: {msg}");
            let app = &self.ivars().app;
            let _ = app.emit("record-pick-cancelled", CancelledEvent { error: Some(msg) });
            super::session::pick_finished(app);
        }
    }
);

impl PickerObserver {
    fn create(app: AppHandle) -> Retained<Self> {
        let this = Self::alloc().set_ivars(Ivars { app });
        unsafe { msg_send![super(this), init] }
    }
}

thread_local! {
    /// Main-thread only: the observer stays registered for the app's life.
    static OBSERVER: RefCell<Option<Retained<PickerObserver>>> = const { RefCell::new(None) };
}

fn own_bundle_id() -> Option<String> {
    unsafe {
        let cls = AnyClass::get(c"NSBundle")?;
        let main: *mut AnyObject = msg_send![cls, mainBundle];
        if main.is_null() {
            return None;
        }
        let id: Option<Retained<NSString>> = msg_send![main, bundleIdentifier];
        id.map(|s| s.to_string()).filter(|s| !s.is_empty())
    }
}

/// Show the system picker for a display or a window. The answer arrives as
/// `record-source-picked` / `record-pick-cancelled`.
pub fn present(app: &AppHandle, kind: SourceKind) -> Result<(), String> {
    if !picker_available() {
        return Err(super::session::ERR_UNSUPPORTED.into());
    }
    REQUESTED.store(if kind == SourceKind::Window { 1 } else { 0 }, Ordering::SeqCst);
    let app2 = app.clone();
    app.run_on_main_thread(move || {
        let res = objc2::exception::catch(std::panic::AssertUnwindSafe(|| unsafe {
            let picker = SCContentSharingPicker::sharedPicker();
            OBSERVER.with(|o| {
                let mut o = o.borrow_mut();
                if o.is_none() {
                    let obs = PickerObserver::create(app2.clone());
                    picker.addObserver(ProtocolObject::from_ref(&*obs));
                    *o = Some(obs);
                }
            });
            let cfg = SCContentSharingPickerConfiguration::new();
            let (mode, style) = match kind {
                SourceKind::Window => (SCContentSharingPickerMode::SingleWindow, SCShareableContentStyle::Window),
                SourceKind::Display => (SCContentSharingPickerMode::SingleDisplay, SCShareableContentStyle::Display),
            };
            cfg.setAllowedPickerModes(mode);
            cfg.setAllowsChangingSelectedContent(false);
            let excludes = match own_bundle_id() {
                Some(bid) => {
                    let ids = NSArray::from_retained_slice(&[NSString::from_str(&bid)]);
                    cfg.setExcludedBundleIDs(&ids);
                    true
                }
                None => false,
            };
            EXCLUDES_APP.store(excludes, Ordering::SeqCst);
            picker.setActive(false);
            picker.setDefaultConfiguration(&cfg);
            picker.setActive(true);
            AWAITING.store(true, Ordering::SeqCst);
            picker.presentPickerUsingContentStyle(style);
        }));
        if let Err(e) = res {
            AWAITING.store(false, Ordering::SeqCst);
            let msg = e.map(|e| format!("{e:?}")).unwrap_or_else(|| "exception".into());
            tracing::warn!("record: presenting the picker failed: {msg}");
            let _ = app2.emit("record-pick-cancelled", CancelledEvent { error: Some(msg) });
            super::session::pick_finished(&app2);
        }
    })
    .map_err(|e| format!("{}:{e}", super::session::ERR_START_FAILED))
}
