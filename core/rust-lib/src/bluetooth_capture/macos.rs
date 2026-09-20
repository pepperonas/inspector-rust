//! macOS live-capture backend: a CoreBluetooth BLE **advertisement scanner**.
//!
//! ⚠️ HONEST SCOPE (§31): macOS exposes no public promiscuous-HCI capture API,
//! so this is NOT a wire sniffer of another app's traffic. It uses the public
//! CoreBluetooth API to scan for advertising BLE devices and logs each
//! advertisement (device id, name, RSSI, service UUIDs, manufacturer-data
//! bytes) as a live packet — a real, useful capability for beacon/sensor
//! reverse-engineering (watch a device's manufacturer-data bytes change via the
//! byte-diff). CoreBluetooth also hides the hardware MAC (it hands out a
//! per-app identifier), which suits the privacy stance (§21).
//!
//! ⚠️ VERIFICATION STATUS: this FFI is compile-verified. Runtime verification
//! needs a Mac with Bluetooth on + the Bluetooth permission granted + a nearby
//! advertising device — none of which exist in the headless build/CI
//! environment. The pure logic it drives (packet mapping, state classification,
//! buffering, batching) IS unit-tested in `capture.rs`/`live.rs`.
//!
//! Threading: the `CBCentralManager` is created with a nil queue, so its
//! delegate callbacks arrive on the MAIN run loop (which Tauri pumps). The
//! retained objects live in main-thread TLS and are only ever touched via
//! `run_on_main_thread`, so nothing non-`Send` crosses a thread boundary.

use std::cell::RefCell;
use std::sync::Arc;

use objc2::rc::Retained;
use objc2::runtime::{AnyObject, NSObject, NSObjectProtocol, ProtocolObject};
use objc2::{define_class, msg_send, AnyThread, DefinedClass};
use objc2_core_bluetooth::{CBCentralManager, CBCentralManagerDelegate, CBPeripheral};
use objc2_foundation::{NSDictionary, NSNumber, NSString};
use tauri::AppHandle;

use super::capture::{self, CbReadiness};
use super::live::{BackendAvailability, LiveShared, SetupStatus};
use super::models::CaptureState;

pub fn setup_status() -> SetupStatus {
    SetupStatus {
        availability: BackendAvailability::Available,
        message: "Scans for advertising Bluetooth LE devices via CoreBluetooth. \
                  Requires Bluetooth to be on and the Bluetooth permission granted."
            .into(),
        source_label: "System Bluetooth (BLE advertisements)".into(),
    }
}

// The advertisement-dictionary keys we read. Their public constant symbols are
// not guaranteed exported by the binding across versions, so we build them from
// the documented, ABI-stable key values (the values BEHIND the public
// `CBAdvertisementData*Key` constants).
fn ns(s: &str) -> Retained<NSString> {
    NSString::from_str(s)
}

thread_local! {
    static SCAN: RefCell<Option<ScanObjects>> = const { RefCell::new(None) };
}

struct ScanObjects {
    central: Retained<CBCentralManager>,
    _delegate: Retained<ScanDelegate>,
}

define_class!(
    #[unsafe(super(NSObject))]
    #[name = "IRBtScanDelegate"]
    #[ivars = Arc<LiveShared>]
    struct ScanDelegate;

    unsafe impl NSObjectProtocol for ScanDelegate {}

    unsafe impl CBCentralManagerDelegate for ScanDelegate {
        #[unsafe(method(centralManagerDidUpdateState:))]
        fn did_update_state(&self, central: &CBCentralManager) {
            self.on_state(central);
        }

        #[unsafe(method(centralManager:didDiscoverPeripheral:advertisementData:RSSI:))]
        fn did_discover(
            &self,
            _central: &CBCentralManager,
            peripheral: &CBPeripheral,
            advertisement_data: &NSDictionary<NSString, AnyObject>,
            rssi: &NSNumber,
        ) {
            self.on_discover(peripheral, advertisement_data, rssi);
        }
    }
);

impl ScanDelegate {
    fn shared(&self) -> &Arc<LiveShared> {
        self.ivars()
    }

    fn on_state(&self, central: &CBCentralManager) {
        // Read the CBManagerState as its raw ordinal (ABI-stable) so we don't
        // depend on the enum spelling, then classify with the pure helper.
        let raw: isize = unsafe { msg_send![central, state] };
        match capture::cb_readiness(raw) {
            CbReadiness::Ready => {
                // powered on → begin scanning for all advertisers
                let nil: *const AnyObject = std::ptr::null();
                let _: () = unsafe {
                    msg_send![central, scanForPeripheralsWithServices: nil, options: nil]
                };
                let _ = self.shared().transition(CaptureState::Capturing);
            }
            CbReadiness::Wait => { /* transient — a newer state callback follows */ }
            CbReadiness::Error(msg) => {
                self.shared().set_error(msg);
            }
        }
    }

    fn on_discover(
        &self,
        peripheral: &CBPeripheral,
        advertisement_data: &NSDictionary<NSString, AnyObject>,
        rssi: &NSNumber,
    ) {
        // Only log while actually capturing (not while paused-stopping/error).
        if self.shared().state() != CaptureState::Capturing {
            return;
        }

        // Device identifier (CoreBluetooth's per-app UUID, never the MAC).
        let identifier = unsafe {
            let uuid: *mut AnyObject = msg_send![peripheral, identifier];
            if uuid.is_null() {
                return; // no id → nothing to key a device on
            }
            let s: *mut NSString = msg_send![uuid, UUIDString];
            nsstring(s).unwrap_or_default()
        };

        // Local name: prefer the advertisement's local name, else the peripheral name.
        let name = read_string(advertisement_data, "kCBAdvDataLocalName")
            .or_else(|| unsafe { nsstring(msg_send![peripheral, name]) });

        let rssi_val: i32 = {
            let v: isize = unsafe { msg_send![rssi, integerValue] };
            v as i32
        };

        let service_uuids = read_service_uuids(advertisement_data);
        let manufacturer = read_data(advertisement_data, "kCBAdvDataManufacturerData");
        let connectable = read_bool(advertisement_data, "kCBAdvDataIsConnectable");

        let abs_ms = now_ms();
        let packet = capture::advertisement_packet(
            name.as_deref(),
            &identifier,
            rssi_val,
            &service_uuids,
            manufacturer,
            connectable,
            abs_ms,
        );
        if let Ok(mut store) = self.shared().store.lock() {
            store.push(packet);
        }
    }
}

/// Start the CoreBluetooth scan on the main thread. The delegate's state
/// callback drives the transition to `Capturing` (or `Error`). Returns
/// immediately; packets arrive asynchronously.
pub fn start(app: &AppHandle, shared: Arc<LiveShared>) -> Result<(), String> {
    app.run_on_main_thread(move || {
        SCAN.with(|slot| {
            if slot.borrow().is_some() {
                return; // already scanning
            }
            let delegate = ScanDelegate::alloc().set_ivars(shared.clone());
            let delegate: Retained<ScanDelegate> = unsafe { msg_send![super(delegate), init] };
            let proto: &ProtocolObject<dyn CBCentralManagerDelegate> =
                ProtocolObject::from_ref(&*delegate);
            let nil: *const AnyObject = std::ptr::null();
            let central: Retained<CBCentralManager> = unsafe {
                let alloc = CBCentralManager::alloc();
                msg_send![alloc, initWithDelegate: proto, queue: nil]
            };
            *slot.borrow_mut() = Some(ScanObjects {
                central,
                _delegate: delegate,
            });
        });
    })
    .map_err(|e| format!("could not start CoreBluetooth on the main thread: {e}"))
}

/// Stop scanning + release the CoreBluetooth objects on the main thread. No-op
/// if not scanning. Guarantees no orphaned central manager / delegate (§20).
pub fn stop(app: &AppHandle) -> Result<(), String> {
    app.run_on_main_thread(|| {
        SCAN.with(|slot| {
            if let Some(obj) = slot.borrow_mut().take() {
                let _: () = unsafe { msg_send![&*obj.central, stopScan] };
                // dropping `obj` releases the central manager + delegate
            }
        });
    })
    .map_err(|e| format!("could not stop CoreBluetooth on the main thread: {e}"))
}

// ── small NSObject extraction helpers (null-safe) ──

unsafe fn nsstring(ptr: *mut NSString) -> Option<String> {
    if ptr.is_null() {
        None
    } else {
        Some((*ptr).to_string())
    }
}

fn read_string(dict: &NSDictionary<NSString, AnyObject>, key: &str) -> Option<String> {
    unsafe {
        let k = ns(key);
        let obj: *mut NSString = msg_send![dict, objectForKey: &*k];
        nsstring(obj)
    }
}

fn read_data(dict: &NSDictionary<NSString, AnyObject>, key: &str) -> Vec<u8> {
    unsafe {
        let k = ns(key);
        let data: *mut AnyObject = msg_send![dict, objectForKey: &*k];
        if data.is_null() {
            return Vec::new();
        }
        let len: usize = msg_send![data, length];
        if len == 0 {
            return Vec::new();
        }
        let bytes: *const u8 = msg_send![data, bytes];
        if bytes.is_null() {
            return Vec::new();
        }
        std::slice::from_raw_parts(bytes, len).to_vec()
    }
}

fn read_bool(dict: &NSDictionary<NSString, AnyObject>, key: &str) -> bool {
    unsafe {
        let k = ns(key);
        let num: *mut AnyObject = msg_send![dict, objectForKey: &*k];
        if num.is_null() {
            return false;
        }
        let b: bool = msg_send![num, boolValue];
        b
    }
}

fn read_service_uuids(dict: &NSDictionary<NSString, AnyObject>) -> Vec<String> {
    unsafe {
        let k = ns("kCBAdvDataServiceUUIDs");
        let arr: *mut AnyObject = msg_send![dict, objectForKey: &*k];
        if arr.is_null() {
            return Vec::new();
        }
        let count: usize = msg_send![arr, count];
        let mut out = Vec::with_capacity(count);
        for i in 0..count {
            let uuid: *mut AnyObject = msg_send![arr, objectAtIndex: i];
            if uuid.is_null() {
                continue;
            }
            let s: *mut NSString = msg_send![uuid, UUIDString];
            if let Some(text) = nsstring(s) {
                out.push(text);
            }
        }
        out
    }
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}
