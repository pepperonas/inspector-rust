//! macOS readers for `pulse` — every value comes from a direct kernel/IOKit
//! call; nothing in here spawns a process, so the 5-s sampling loop stays
//! cheap (the brief: no shell calls inside the loop).
//!
//! Every `unsafe` block carries a `// SAFETY:` note. The common shape: we pass
//! a correctly sized, zero-initialised buffer to a C function that fills it,
//! and we only read the buffer after the call reports success.

use std::collections::HashMap;
use std::ffi::{c_char, c_int, c_void, CStr, CString};

use super::{CpuTicks, DiskCandidate, ProcReading, SystemReading};

// ── sysctl helpers ─────────────────────────────────────────────────────────

fn sysctl_raw<T: Default + Copy>(name: &str) -> Option<T> {
    let cname = CString::new(name).ok()?;
    let mut value = T::default();
    let mut len = std::mem::size_of::<T>();
    // SAFETY: `value` is a properly aligned T and `len` is its exact size; the
    // kernel writes at most `len` bytes and reports the written size back.
    let rc = unsafe {
        libc::sysctlbyname(
            cname.as_ptr(),
            &mut value as *mut T as *mut c_void,
            &mut len,
            std::ptr::null_mut(),
            0,
        )
    };
    (rc == 0 && len == std::mem::size_of::<T>()).then_some(value)
}

/// `kern.memorystatus_vm_pressure_level`: 1 normal, 2 warn, 4 critical.
fn pressure_level() -> Option<u8> {
    sysctl_raw::<c_int>("kern.memorystatus_vm_pressure_level").map(|v| v.clamp(0, 255) as u8)
}

#[derive(Default, Clone, Copy)]
#[repr(C)]
struct XswUsage {
    total: u64,
    avail: u64,
    used: u64,
    pagesize: u32,
    encrypted: u32,
}

fn swap_usage() -> Option<(u64, u64)> {
    // libc's xsw_usage lacks Default; a local mirror with the same layout
    // (u64,u64,u64,u32,boolean_t) is read through the same sysctl.
    sysctl_raw::<XswUsage>("vm.swapusage").map(|x| (x.used, x.total))
}

fn page_size() -> u64 {
    // SAFETY: `vm_page_size` is a process-global set by the kernel at exec time
    // and never written afterwards; reading it is a plain load.
    let ps = unsafe { libc::vm_page_size } as u64;
    if ps == 0 {
        16_384
    } else {
        ps
    }
}

extern "C" {
    // Declared here instead of via libc (deprecated there in favour of mach2).
    fn mach_host_self() -> u32;
    fn mach_thread_self() -> u32;
}

/// The host port, fetched ONCE: every `mach_host_self()` call adds a user
/// reference to the send right, and calling it twice per 5-s tick would walk
/// the reference count towards its overflow over days of uptime.
fn host_port() -> u32 {
    use std::sync::OnceLock;
    static PORT: OnceLock<u32> = OnceLock::new();
    // SAFETY: no arguments; returns a send right we deliberately keep for the
    // process lifetime.
    *PORT.get_or_init(|| unsafe { mach_host_self() })
}

fn vm_stats() -> Option<libc::vm_statistics64> {
    // SAFETY: zeroed is a valid bit pattern for this plain-integer C struct.
    let mut stats: libc::vm_statistics64 = unsafe { std::mem::zeroed() };
    let mut count = libc::HOST_VM_INFO64_COUNT;
    // SAFETY: the buffer is a full vm_statistics64 and `count` is its size in
    // integer_t units, exactly what host_statistics64 expects.
    let rc = unsafe {
        libc::host_statistics64(
            host_port(),
            libc::HOST_VM_INFO64,
            &mut stats as *mut _ as *mut libc::integer_t,
            &mut count,
        )
    };
    (rc == 0).then_some(stats)
}

fn cpu_ticks() -> Option<CpuTicks> {
    let mut info = [0u32; 4];
    let mut count = libc::HOST_CPU_LOAD_INFO_COUNT;
    // SAFETY: HOST_CPU_LOAD_INFO fills CPU_STATE_MAX (4) natural_t counters,
    // and `info` is exactly that size.
    let rc = unsafe {
        libc::host_statistics(
            host_port(),
            libc::HOST_CPU_LOAD_INFO,
            info.as_mut_ptr() as *mut libc::integer_t,
            &mut count,
        )
    };
    // Order: user, system, idle, nice.
    (rc == 0).then_some(CpuTicks {
        busy: info[0] as u64 + info[1] as u64 + info[3] as u64,
        total: info.iter().map(|&v| v as u64).sum(),
    })
}

/// NSProcessInfo.thermalState: 0 nominal, 1 fair, 2 serious, 3 critical.
fn thermal_state() -> u8 {
    use objc2::runtime::{AnyClass, AnyObject};
    let Some(cls) = AnyClass::get(c"NSProcessInfo") else { return 0 };
    // SAFETY: +processInfo returns the shared, never-nil singleton and
    // -thermalState is a side-effect-free NSInteger getter (macOS 10.10.3+).
    unsafe {
        let info: *mut AnyObject = objc2::msg_send![cls, processInfo];
        if info.is_null() {
            return 0;
        }
        let state: isize = objc2::msg_send![info, thermalState];
        state.clamp(0, 3) as u8
    }
}

pub fn read_system() -> SystemReading {
    let vm = vm_stats();
    let page = page_size();
    let (swap_used, swap_total) = swap_usage().unwrap_or((0, 0));
    SystemReading {
        pressure: pressure_level().unwrap_or(0),
        swap_used,
        swap_total,
        page_size: page,
        swapouts: vm.map(|v| v.swapouts),
        swapins: vm.map(|v| v.swapins),
        pageouts: vm.map(|v| v.pageouts),
        compressions: vm.map(|v| v.compressions),
        decompressions: vm.map(|v| v.decompressions),
        wired_bytes: vm.map(|v| v.wire_count as u64 * page).unwrap_or(0),
        active_bytes: vm.map(|v| v.active_count as u64 * page).unwrap_or(0),
        inactive_bytes: vm.map(|v| v.inactive_count as u64 * page).unwrap_or(0),
        free_bytes: vm.map(|v| v.free_count as u64 * page).unwrap_or(0),
        compressor_bytes: vm.map(|v| v.compressor_page_count as u64 * page).unwrap_or(0),
        cpu: cpu_ticks(),
        thermal: thermal_state(),
        disks: disk_candidates(),
    }
}

// ── Own CPU time (for the overhead report) ─────────────────────────────────

/// The calling thread's port. Like `mach_host_self`, every call adds a user
/// reference — call it once per thread and keep the value.
pub fn current_thread_port() -> u32 {
    // SAFETY: no arguments; returns a send right to the calling thread.
    unsafe { mach_thread_self() }
}

/// User + system CPU seconds the given thread has consumed so far.
pub fn thread_cpu_secs(port: u32) -> Option<f64> {
    // SAFETY: zeroed thread_basic_info is valid; the count is its size in
    // integer_t units, and we read the struct only when the call succeeds.
    let mut info: libc::thread_basic_info = unsafe { std::mem::zeroed() };
    let mut count = libc::THREAD_BASIC_INFO_COUNT;
    let rc = unsafe {
        libc::thread_info(
            port,
            libc::THREAD_BASIC_INFO as u32,
            &mut info as *mut _ as libc::thread_info_t,
            &mut count,
        )
    };
    if rc != 0 {
        return None;
    }
    let t = |v: libc::time_value_t| v.seconds as f64 + v.microseconds as f64 / 1e6;
    Some(t(info.user_time) + t(info.system_time))
}

// ── IOKit: block-storage statistics ────────────────────────────────────────

type CfRef = *const c_void;

#[link(name = "IOKit", kind = "framework")]
extern "C" {
    fn IOServiceMatching(name: *const c_char) -> *mut c_void;
    fn IOServiceGetMatchingServices(master: u32, matching: *mut c_void, iter: *mut u32) -> i32;
    fn IOIteratorNext(iter: u32) -> u32;
    fn IOObjectRelease(obj: u32) -> i32;
    fn IORegistryEntryCreateCFProperty(entry: u32, key: CfRef, alloc: CfRef, opts: u32) -> CfRef;
    fn IORegistryEntrySearchCFProperty(
        entry: u32,
        plane: *const c_char,
        key: CfRef,
        alloc: CfRef,
        opts: u32,
    ) -> CfRef;
    fn IORegistryEntryGetRegistryEntryID(entry: u32, id: *mut u64) -> i32;
}

#[link(name = "CoreFoundation", kind = "framework")]
extern "C" {
    fn CFStringCreateWithCString(alloc: CfRef, s: *const c_char, enc: u32) -> CfRef;
    fn CFStringGetCString(s: CfRef, buf: *mut c_char, size: isize, enc: u32) -> bool;
    fn CFDictionaryGetValue(dict: CfRef, key: CfRef) -> CfRef;
    fn CFNumberGetValue(num: CfRef, kind: i32, out: *mut c_void) -> bool;
    fn CFGetTypeID(cf: CfRef) -> usize;
    fn CFStringGetTypeID() -> usize;
    fn CFNumberGetTypeID() -> usize;
    fn CFDictionaryGetTypeID() -> usize;
    fn CFRelease(cf: CfRef);
}

const UTF8: u32 = 0x0800_0100;
const CF_NUMBER_SINT64: i32 = 4;
const ITERATE_RECURSIVELY: u32 = 1;
const ITERATE_PARENTS: u32 = 2;

/// An owned CFString key, released on drop.
struct CfStr(CfRef);
impl CfStr {
    fn new(s: &str) -> Option<Self> {
        let c = CString::new(s).ok()?;
        // SAFETY: valid NUL-terminated UTF-8; the result is +1 retained and
        // released in Drop.
        let r = unsafe { CFStringCreateWithCString(std::ptr::null(), c.as_ptr(), UTF8) };
        (!r.is_null()).then_some(Self(r))
    }
}
impl Drop for CfStr {
    fn drop(&mut self) {
        // SAFETY: we own exactly one retain on this non-null object.
        unsafe { CFRelease(self.0) }
    }
}

/// Owned CF object (+1), released on drop.
struct Owned(CfRef);
impl Drop for Owned {
    fn drop(&mut self) {
        if !self.0.is_null() {
            // SAFETY: created by a Copy/Create function → we own one retain.
            unsafe { CFRelease(self.0) }
        }
    }
}

/// Read a borrowed CF value as i64 if it is a CFNumber.
fn cf_i64(v: CfRef) -> Option<i64> {
    if v.is_null() {
        return None;
    }
    // SAFETY: type-checked with CFGetTypeID before reading; `out` is an i64
    // matching kCFNumberSInt64Type.
    unsafe {
        if CFGetTypeID(v) != CFNumberGetTypeID() {
            return None;
        }
        let mut out: i64 = 0;
        CFNumberGetValue(v, CF_NUMBER_SINT64, &mut out as *mut i64 as *mut c_void).then_some(out)
    }
}

fn cf_string(v: CfRef) -> Option<String> {
    if v.is_null() {
        return None;
    }
    let mut buf = [0 as c_char; 128];
    // SAFETY: type-checked; the buffer size is passed so the copy is bounded
    // and NUL-terminated on success.
    unsafe {
        if CFGetTypeID(v) != CFStringGetTypeID() {
            return None;
        }
        if !CFStringGetCString(v, buf.as_mut_ptr(), buf.len() as isize, UTF8) {
            return None;
        }
        Some(CStr::from_ptr(buf.as_ptr()).to_string_lossy().into_owned())
    }
}

fn dict_get(dict: CfRef, key: &str) -> CfRef {
    let Some(k) = CfStr::new(key) else { return std::ptr::null() };
    // SAFETY: `dict` was type-checked as a CFDictionary by the caller; the
    // returned value is borrowed (Get rule) and only used while `dict` lives.
    unsafe { CFDictionaryGetValue(dict, k.0) }
}

fn is_dict(v: CfRef) -> bool {
    // SAFETY: CFGetTypeID accepts any non-null CF object.
    !v.is_null() && unsafe { CFGetTypeID(v) == CFDictionaryGetTypeID() }
}

/// Every IOBlockStorageDriver with its cumulative byte counters and where the
/// device sits; `pick_internal_disk` (pure) chooses the one to follow.
fn disk_candidates() -> Vec<DiskCandidate> {
    let mut out = Vec::new();
    let Ok(class) = CString::new("IOBlockStorageDriver") else { return out };
    let mut iter: u32 = 0;
    // SAFETY: IOServiceMatching returns a +1 dictionary that
    // IOServiceGetMatchingServices consumes; `iter` is released below.
    let kr = unsafe { IOServiceGetMatchingServices(0, IOServiceMatching(class.as_ptr()), &mut iter) };
    if kr != 0 {
        return out;
    }
    let (Some(stats_key), Some(proto_key), Some(size_key)) =
        (CfStr::new("Statistics"), CfStr::new("Protocol Characteristics"), CfStr::new("Size"))
    else {
        // SAFETY: iterator object from the successful call above.
        unsafe { IOObjectRelease(iter) };
        return out;
    };
    let plane = c"IOService";
    loop {
        // SAFETY: walking a valid iterator; 0 ends it.
        let svc = unsafe { IOIteratorNext(iter) };
        if svc == 0 {
            break;
        }
        let mut id: u64 = 0;
        // SAFETY: `svc` is a live registry entry; the property calls return +1
        // objects owned by `Owned`; parents/children searches are read-only.
        unsafe {
            IORegistryEntryGetRegistryEntryID(svc, &mut id);
            let stats = Owned(IORegistryEntryCreateCFProperty(svc, stats_key.0, std::ptr::null(), 0));
            let proto = Owned(IORegistryEntrySearchCFProperty(
                svc,
                plane.as_ptr(),
                proto_key.0,
                std::ptr::null(),
                ITERATE_RECURSIVELY | ITERATE_PARENTS,
            ));
            let size = Owned(IORegistryEntrySearchCFProperty(
                svc,
                plane.as_ptr(),
                size_key.0,
                std::ptr::null(),
                ITERATE_RECURSIVELY,
            ));
            if is_dict(stats.0) {
                let (location, interconnect) = if is_dict(proto.0) {
                    (
                        cf_string(dict_get(proto.0, "Physical Interconnect Location")),
                        cf_string(dict_get(proto.0, "Physical Interconnect")),
                    )
                } else {
                    (None, None)
                };
                out.push(DiskCandidate {
                    id,
                    bytes_written: cf_i64(dict_get(stats.0, "Bytes (Write)")).unwrap_or(0).max(0) as u64,
                    bytes_read: cf_i64(dict_get(stats.0, "Bytes (Read)")).unwrap_or(0).max(0) as u64,
                    location,
                    interconnect,
                    capacity: cf_i64(size.0).map(|v| v.max(0) as u64),
                });
            }
            IOObjectRelease(svc);
        }
    }
    // SAFETY: iterator from the successful call above.
    unsafe { IOObjectRelease(iter) };
    out
}

// ── Processes ──────────────────────────────────────────────────────────────

type RespFn = unsafe extern "C" fn(c_int) -> c_int;

/// `responsibility_get_pid_responsible_for_pid` — undocumented libsystem
/// function Activity Monitor uses to fold helpers into their app. Looked up at
/// runtime so a future macOS that drops it degrades to the name rules.
fn responsible_fn() -> Option<RespFn> {
    use std::sync::OnceLock;
    static F: OnceLock<Option<usize>> = OnceLock::new();
    let addr = *F.get_or_init(|| {
        // SAFETY: dlsym with RTLD_DEFAULT and a static NUL-terminated name.
        let p = unsafe {
            libc::dlsym(libc::RTLD_DEFAULT, c"responsibility_get_pid_responsible_for_pid".as_ptr())
        };
        (!p.is_null()).then_some(p as usize)
    });
    // SAFETY: the symbol has signature pid_t(pid_t) on every macOS that ships it.
    addr.map(|a| unsafe { std::mem::transmute::<usize, RespFn>(a) })
}

fn proc_path(pid: c_int) -> Option<String> {
    let mut buf = vec![0u8; 4096];
    // SAFETY: buffer of PROC_PIDPATHINFO_MAXSIZE bytes; the call writes a
    // NUL-terminated path and returns its length (>0) on success.
    let n = unsafe { libc::proc_pidpath(pid, buf.as_mut_ptr() as *mut c_void, buf.len() as u32) };
    if n <= 0 {
        return None;
    }
    buf.truncate(n as usize);
    Some(String::from_utf8_lossy(&buf).into_owned())
}

fn proc_short_name(pid: c_int) -> Option<String> {
    let mut buf = [0u8; 256];
    // SAFETY: bounded buffer; returns the number of bytes written.
    let n = unsafe { libc::proc_name(pid, buf.as_mut_ptr() as *mut c_void, buf.len() as u32) };
    (n > 0).then(|| String::from_utf8_lossy(&buf[..n as usize]).into_owned())
}

/// argv of a process via KERN_PROCARGS2 (only called for `java`, to tell a
/// Gradle daemon from a Kotlin daemon). Layout: argc (i32), exec path,
/// padding NULs, then argc NUL-terminated arguments.
fn proc_args(pid: c_int) -> Option<Vec<String>> {
    let mut mib = [libc::CTL_KERN, libc::KERN_PROCARGS2, pid];
    let mut size: usize = 0;
    // SAFETY: first call with a null buffer only asks for the size.
    let rc = unsafe {
        libc::sysctl(mib.as_mut_ptr(), 3, std::ptr::null_mut(), &mut size, std::ptr::null_mut(), 0)
    };
    if rc != 0 || !(4..=1 << 20).contains(&size) {
        return None;
    }
    let mut buf = vec![0u8; size];
    // SAFETY: buffer of the size the kernel just reported.
    let rc = unsafe {
        libc::sysctl(mib.as_mut_ptr(), 3, buf.as_mut_ptr() as *mut c_void, &mut size, std::ptr::null_mut(), 0)
    };
    if rc != 0 {
        return None;
    }
    buf.truncate(size);
    super::parse_procargs2(&buf)
}

/// Readable processes of this user with their footprint + cumulative writes.
/// Others (root, other users) fail with EPERM and are only counted.
pub fn read_processes(arg_cache: &mut HashMap<(i32, u64), Option<Vec<String>>>) -> (Vec<ProcReading>, usize) {
    let mut pids = vec![0 as c_int; 4096];
    // SAFETY: buffer size in bytes; returns the number of pids written.
    let n = unsafe {
        libc::proc_listallpids(pids.as_mut_ptr() as *mut c_void, (pids.len() * std::mem::size_of::<c_int>()) as c_int)
    };
    if n <= 0 {
        return (Vec::new(), 0);
    }
    pids.truncate(n as usize);
    let resp = responsible_fn();
    let mut out = Vec::with_capacity(pids.len());
    let mut unreadable = 0usize;
    let mut seen_keys = Vec::new();
    for &pid in &pids {
        if pid <= 0 {
            continue;
        }
        // SAFETY: zeroed rusage_info_v4 is valid; flavor V4 matches the struct
        // passed, and we only read it when the call returns 0.
        let mut ri: libc::rusage_info_v4 = unsafe { std::mem::zeroed() };
        let rc = unsafe {
            libc::proc_pid_rusage(pid, libc::RUSAGE_INFO_V4, &mut ri as *mut _ as *mut libc::rusage_info_t)
        };
        if rc != 0 {
            unreadable += 1;
            continue;
        }
        let name = proc_short_name(pid).unwrap_or_default();
        let path = proc_path(pid);
        // SAFETY: plain pid_t → pid_t call (see responsible_fn).
        let rpid = resp.map(|f| unsafe { f(pid) }).filter(|&r| r > 0 && r != pid);
        let responsible_path = rpid.and_then(proc_path);
        let key = (pid, ri.ri_proc_start_abstime);
        let args = if name == "java" {
            seen_keys.push(key);
            arg_cache.entry(key).or_insert_with(|| proc_args(pid)).clone()
        } else {
            None
        };
        out.push(ProcReading {
            pid,
            start: ri.ri_proc_start_abstime,
            name,
            path,
            responsible_path,
            args,
            footprint: ri.ri_phys_footprint,
            bytes_written: ri.ri_diskio_byteswritten,
        });
    }
    arg_cache.retain(|k, _| seen_keys.contains(k));
    (out, unreadable)
}

#[cfg(test)]
mod live_tests {
    use super::*;

    /// `cargo test -p inspector-rust-core --lib pulse_live -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn pulse_live_readers() {
        let t = std::time::Instant::now();
        let r = read_system();
        let sys_ms = t.elapsed().as_secs_f64() * 1000.0;
        println!(
            "pressure={} swap={:.2}/{:.2} GB page={} swapouts={:?} compressor={:.2} GB thermal={} cpu={:?}",
            r.pressure,
            r.swap_used as f64 / 1e9,
            r.swap_total as f64 / 1e9,
            r.page_size,
            r.swapouts,
            r.compressor_bytes as f64 / 1e9,
            r.thermal,
            r.cpu
        );
        for d in &r.disks {
            println!("disk {:?}", d);
        }
        let pick = super::super::pick_internal_disk(&r.disks);
        println!("picked: {:?}", pick.map(|d| d.id));
        let t = std::time::Instant::now();
        let mut cache = HashMap::new();
        let (procs, unreadable) = read_processes(&mut cache);
        let scan_ms = t.elapsed().as_secs_f64() * 1000.0;
        let (groups, _, _) = super::super::group_scan(&procs, &HashMap::new(), 30.0);
        for g in groups.iter().take(10) {
            println!("{:>8.2} GB  {:>3} procs  {}", g.footprint as f64 / 1e9, g.procs, g.name);
        }
        println!("system read {sys_ms:.2} ms, process scan {scan_ms:.2} ms ({} readable, {unreadable} unreadable)", procs.len());
        assert!(r.page_size >= 4096);
        assert!(r.swapouts.is_some());
        assert!(pick.is_some());
        assert!(!procs.is_empty());
    }
}
