//! System sleep status (macOS): is something holding the machine awake right
//! now — and for how long until idle sleep is possible again?
//!
//! Motivation: a Mac on `pmset sleep 1` is routinely kept awake by short-lived
//! `caffeinate` assertions (Claude Code spawns one per activity burst, 300 s
//! each). The popup footer shows a compact indicator so a glance answers "can
//! I walk away and let it sleep?".
//!
//! Data source is `pmset` (no sudo needed):
//!  * `pmset -g assertions` — one header line per assertion
//!    (`pid 27261(caffeinate): [0x…] 00:00:40 PreventUserIdleSystemSleep
//!    named: "…"`), followed by INDENTED continuation lines, among them
//!    optionally `Timeout will fire in 260 secs`. Continuations belong to the
//!    most recent header, so the parser is a tiny line state machine.
//!  * `pmset -g` — the ACTIVE profile. `sleep 0` = system sleep disabled
//!    outright. Field-verified subtlety: the profiles differ per power source
//!    (this machine: AC `sleep 0`, battery `sleep 1`) and `pmset -g` shows the
//!    one in effect — exactly what the indicator should reflect. The trailing
//!    `(sleep prevented by …)` annotation is ignored; assertions are read from
//!    the dedicated listing instead.
//!
//! ⚠️ FILTER RULE (load-bearing): powerd's "Powerd - Prevent sleep while
//! display is on" assertion is NOT counted as a holder — it is present the
//! whole time the display is on (i.e. always, whenever a user could possibly
//! look at the footer), so counting it would report "prevented ∞" forever and
//! drown the actual signal. caffeinate / coreaudiod / app assertions count.
//!
//! ⚠️ Continuation lines must be attributed carefully — two real-world traps,
//! both present in the captured fixture below:
//!  * a NON-counted assertion type (WindowServer's `UserIsActive`) can carry
//!    its own `Timeout will fire in 600 secs` line — attaching that to the
//!    previous counted assertion would fabricate a wrong countdown;
//!  * a FILTERED assertion (powerd's, or its `InternalPreventDisplaySleep`
//!    sibling with `Timeout … 209 secs`) sits between counted ones — its
//!    timeout must not leak onto e.g. bluetoothd's indefinite assertion.
//!
//! Both are solved the same way: every header line resets the "current
//! assertion" slot; only a counted header occupies it.
//!
//! House style: the parsers + aggregation are pure and unit-tested against
//! real `pmset` output captured from this machine; the process spawn is a thin
//! impure shell (`current()`), untested.

/// One counted sleep-preventing assertion.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Assertion {
    /// Owning process name, e.g. `caffeinate`.
    pub process: String,
    /// Seconds until the assertion self-releases; `None` = indefinite (held
    /// until the process drops it or exits).
    pub timeout_secs: Option<u64>,
}

/// What the footer indicator renders. `supported == false` (non-macOS, or
/// `pmset` failed/unparseable) hides the indicator entirely — a broken probe
/// must never masquerade as "nothing is preventing sleep".
#[derive(Debug, Clone, serde::Serialize)]
pub struct SleepStatus {
    pub supported: bool,
    /// The ACTIVE pmset profile has `sleep 0` — idle sleep never happens,
    /// regardless of assertions.
    pub sleep_disabled: bool,
    /// At least one counted assertion is holding the system awake.
    pub prevented: bool,
    /// At least one counted assertion has no timeout.
    pub indefinite: bool,
    /// Largest assertion timeout = seconds until idle sleep is possible again
    /// (once every timed assertion has fired). `None` when nothing timed.
    pub max_timeout_secs: Option<u64>,
    /// Deduplicated holder names with counts, first-seen order —
    /// e.g. `["caffeinate ×4", "sharingd"]`. Tooltip material.
    pub holders: Vec<String>,
}

impl SleepStatus {
    fn unsupported() -> Self {
        SleepStatus {
            supported: false,
            sleep_disabled: false,
            prevented: false,
            indefinite: false,
            max_timeout_secs: None,
            holders: Vec::new(),
        }
    }
}

/// The assertion type we count. Everything else (`UserIsActive`,
/// `InternalPreventDisplaySleep`, `PreventUserIdleDisplaySleep`, …) merely
/// affects the display or is bookkeeping.
const COUNTED_KIND: &str = "PreventUserIdleSystemSleep";

/// Parse `pmset -g assertions` into the counted assertions. Tolerant by
/// construction: any line that isn't a recognisable header or a
/// `Timeout will fire in N secs` continuation is skipped (the surrounding
/// format — summary table, kernel assertions, `Details:`/`Localized=` lines —
/// varies across macOS versions).
pub fn parse_assertions(out: &str) -> Vec<Assertion> {
    let mut result: Vec<Assertion> = Vec::new();
    // Index into `result` of the assertion continuation lines belong to;
    // `None` after a non-counted or filtered header (see module doc).
    let mut current: Option<usize> = None;

    for line in out.lines() {
        let trimmed = line.trim();
        if let Some(header) = parse_header(trimmed) {
            match header {
                Header::Counted(a) => {
                    result.push(a);
                    current = Some(result.len() - 1);
                }
                Header::Other => current = None,
            }
            continue;
        }
        if let Some(secs) = parse_timeout_line(trimmed) {
            if let Some(i) = current {
                result[i].timeout_secs = Some(secs);
            }
        }
    }
    result
}

enum Header {
    /// A `PreventUserIdleSystemSleep` assertion that passed the powerd filter.
    Counted(Assertion),
    /// Any other assertion header — resets continuation attribution.
    Other,
}

/// `pid 27261(caffeinate): [0x…] 00:00:40 PreventUserIdleSystemSleep named: "…"`
/// → its process + whether it counts. `None` = not an assertion header at all.
fn parse_header(trimmed: &str) -> Option<Header> {
    let rest = trimmed.strip_prefix("pid ")?;
    // `31809(caffeinate): …` — digits, then the process in parentheses.
    let open = rest.find('(')?;
    if !rest[..open].chars().all(|c| c.is_ascii_digit()) || rest[..open].is_empty() {
        return None;
    }
    let close = rest[open..].find(')')? + open;
    let process = &rest[open + 1..close];
    // Only lines that name an assertion are headers (`named:` is the marker —
    // it is what separates them from e.g. the kernel-assertion `id=…` lines).
    let named_at = rest.find(" named:")?;
    if !rest[..named_at].contains(COUNTED_KIND) {
        return Some(Header::Other);
    }
    // The powerd display-on assertion would report "prevented" forever — see
    // the module-level FILTER RULE. Matched loosely (process + name fragment)
    // to survive wording drift.
    let name = rest[named_at..].split('"').nth(1).unwrap_or("");
    if process == "powerd" && name.contains("Prevent sleep while display is on") {
        return Some(Header::Other);
    }
    Some(Header::Counted(Assertion {
        process: process.to_string(),
        timeout_secs: None,
    }))
}

/// `Timeout will fire in 260 secs Action=TimeoutActionRelease` → `260`.
fn parse_timeout_line(trimmed: &str) -> Option<u64> {
    let rest = trimmed.strip_prefix("Timeout will fire in ")?;
    let digits: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
    let secs: u64 = digits.parse().ok()?;
    rest[digits.len()..].trim_start().starts_with("secs").then_some(secs)
}

/// Parse the active profile's `sleep` value out of `pmset -g`. The line reads
/// ` sleep                0 (sleep prevented by …)` — value first, optional
/// annotation after. Case-sensitive `sleep ` prefix keeps `displaysleep`,
/// `disksleep` and `Sleep On Power Button` out.
pub fn parse_profile_sleep(out: &str) -> Option<u64> {
    for line in out.lines() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix("sleep ") {
            return rest.split_whitespace().next()?.parse().ok();
        }
    }
    None
}

/// Pure aggregation of the two parses into the wire struct.
pub fn status_from(assertions: &[Assertion], profile_sleep: Option<u64>) -> SleepStatus {
    let mut holders: Vec<(String, usize)> = Vec::new();
    for a in assertions {
        match holders.iter_mut().find(|(name, _)| *name == a.process) {
            Some((_, n)) => *n += 1,
            None => holders.push((a.process.clone(), 1)),
        }
    }
    SleepStatus {
        supported: true,
        sleep_disabled: profile_sleep == Some(0),
        prevented: !assertions.is_empty(),
        indefinite: assertions.iter().any(|a| a.timeout_secs.is_none()),
        max_timeout_secs: assertions.iter().filter_map(|a| a.timeout_secs).max(),
        holders: holders
            .into_iter()
            .map(|(name, n)| if n > 1 { format!("{name} ×{n}") } else { name })
            .collect(),
    }
}

/// Impure shell: spawn `pmset` twice and aggregate. macOS only; any spawn
/// failure → `unsupported` (hide the indicator rather than claim a state).
/// Callers run this off the main thread (subprocess spawn — see the async-IPC
/// invariant in CLAUDE.md).
#[cfg(target_os = "macos")]
pub fn current() -> SleepStatus {
    fn pmset(args: &[&str]) -> Option<String> {
        let out = std::process::Command::new("/usr/bin/pmset").args(args).output().ok()?;
        out.status.success().then(|| String::from_utf8_lossy(&out.stdout).into_owned())
    }
    let Some(assertions_out) = pmset(&["-g", "assertions"]) else {
        return SleepStatus::unsupported();
    };
    // The profile read is best-effort: without it we still know the assertion
    // side, only `sleep_disabled` degrades to false.
    let profile = pmset(&["-g"]).as_deref().and_then(parse_profile_sleep);
    status_from(&parse_assertions(&assertions_out), profile)
}

#[cfg(not(target_os = "macos"))]
pub fn current() -> SleepStatus {
    SleepStatus::unsupported()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Real `pmset -g assertions` output captured from this machine
    /// (2026-08-23, trimmed to the representative middle). It contains every
    /// trap the parser must survive: the summary table (mentions the counted
    /// kind without being a header), caffeinate assertions with tab-indented
    /// `Details:`/`Localized=`/`Timeout` continuations, sharingd + bluetoothd
    /// WITHOUT timeouts (= indefinite), WindowServer's NON-counted
    /// `UserIsActive` with its own 600 s timeout, powerd's FILTERED display-on
    /// assertion followed by its `InternalPreventDisplaySleep` sibling with a
    /// 209 s timeout, and the kernel-assertion tail.
    const REAL_ASSERTIONS: &str = "2026-08-23 01:54:21 +0200 \n\
Assertion status system-wide:\n\
   BackgroundTask                 0\n\
   UserIsActive                   1\n\
   PreventUserIdleSystemSleep     1\n\
Listed by owning process:\n\
   pid 31809(caffeinate): [0x000d4c09000196fd] 00:01:03 PreventUserIdleSystemSleep named: \"caffeinate command-line tool\"  \n\
\tDetails: caffeinate asserting for 300 secs\n\
\tLocalized=THE CAFFEINATE TOOL IS PREVENTING SLEEP.\n\
\tTimeout will fire in 237 secs Action=TimeoutActionRelease\n\
   pid 707(sharingd): [0x000d4c08000196fb] 00:01:04 PreventUserIdleSystemSleep named: \"Handoff\"  \n\
   pid 32073(caffeinate): [0x000d4c2f00019702] 00:00:25 PreventUserIdleSystemSleep named: \"caffeinate command-line tool\"  \n\
\tDetails: caffeinate asserting for 300 secs\n\
\tLocalized=THE CAFFEINATE TOOL IS PREVENTING SLEEP.\n\
\tTimeout will fire in 274 secs Action=TimeoutActionRelease\n\
   pid 446(WindowServer): [0x000d33f400099066] 00:00:00 UserIsActive named: \"com.apple.iohideventsystem.queue.tickle serviceID:100114df8 service:AppleUserHIDEventService product:MX Master eventType:17\"  \n\
\tTimeout will fire in 600 secs Action=TimeoutActionRelease\n\
   pid 438(bluetoothd): [0x000d4c4100019704] 00:00:07 PreventUserIdleSystemSleep named: \"com.apple.BTStack\"  \n\
   pid 386(powerd): [0x000d33f20001905c] 01:43:50 PreventUserIdleSystemSleep named: \"Powerd - Prevent sleep while display is on\"  \n\
   pid 386(powerd): [0x000d4b4900108799] 00:01:31 InternalPreventDisplaySleep named: \"com.apple.powermanagement.delayDisplayOff\"  \n\
\tTimeout will fire in 209 secs Action=TimeoutActionTurnOff\n\
   pid 30995(caffeinate): [0x000d4b7e000196e3] 00:03:22 PreventUserIdleSystemSleep named: \"caffeinate command-line tool\"  \n\
\tDetails: caffeinate asserting for 300 secs\n\
\tLocalized=THE CAFFEINATE TOOL IS PREVENTING SLEEP.\n\
\tTimeout will fire in 98 secs Action=TimeoutActionRelease\n\
Kernel Assertions: 0x100=MAGICWAKE\n\
   id=558  level=255 0x100=MAGICWAKE creat=07.08.26, 19:23  mod=22.08.26, 08:15 description=en0 owner=IOSkywalkNetworkBSDClient\n";

    /// Real `pmset -g` capture (AC profile of this machine — `sleep 0` is the
    /// genuinely stored value, verified against `pmset -g custom`; the
    /// annotation is decoration the parser must see straight through).
    const REAL_PROFILE_AC: &str = "System-wide power settings:\n\
Currently in use:\n\
 standby              1\n\
 Sleep On Power Button 1\n\
 disksleep            10\n\
 sleep                0 (sleep prevented by caffeinate, sharingd, powerd)\n\
 displaysleep         10\n\
 ttyskeepawake        1\n";

    #[test]
    fn parses_the_real_capture_with_correct_attribution() {
        let a = parse_assertions(REAL_ASSERTIONS);
        // 3 caffeinate + sharingd + bluetoothd; powerd filtered, UserIsActive
        // and InternalPreventDisplaySleep are other kinds.
        assert_eq!(a.len(), 5);
        let timeouts: Vec<Option<u64>> = a.iter().map(|x| x.timeout_secs).collect();
        assert_eq!(
            timeouts,
            vec![Some(237), None, Some(274), None, Some(98)],
            "each timeout on ITS OWN assertion — and neither WindowServer's 600 \
             nor powerd's 209 leaked onto a counted one"
        );
        assert_eq!(a[1].process, "sharingd");
        assert_eq!(a[3].process, "bluetoothd");
    }

    #[test]
    fn a_non_counted_header_resets_continuation_attribution() {
        // Distilled trap: counted assertion WITHOUT its own timeout, then a
        // non-counted assertion WITH one. The 600 must go nowhere.
        let out = "   pid 707(sharingd): [0x1] 00:01:04 PreventUserIdleSystemSleep named: \"Handoff\"\n\
   pid 446(WindowServer): [0x2] 00:00:00 UserIsActive named: \"tickle\"\n\
\tTimeout will fire in 600 secs Action=TimeoutActionRelease\n";
        let a = parse_assertions(out);
        assert_eq!(a.len(), 1);
        assert_eq!(a[0].timeout_secs, None, "sharingd must stay indefinite");
    }

    #[test]
    fn the_powerd_display_assertion_is_filtered_and_resets_attribution() {
        let out = "   pid 438(bluetoothd): [0x1] 00:00:07 PreventUserIdleSystemSleep named: \"com.apple.BTStack\"\n\
   pid 386(powerd): [0x2] 01:43:50 PreventUserIdleSystemSleep named: \"Powerd - Prevent sleep while display is on\"\n\
\tTimeout will fire in 209 secs Action=TimeoutActionTurnOff\n";
        let a = parse_assertions(out);
        assert_eq!(a.len(), 1, "powerd's display-on assertion never counts");
        assert_eq!(a[0].process, "bluetoothd");
        assert_eq!(a[0].timeout_secs, None, "the filtered assertion's timeout must not leak");
    }

    #[test]
    fn a_powerd_assertion_with_another_name_still_counts() {
        // The filter targets the one always-on display assertion, not the
        // process — powerd can legitimately hold others (e.g. wake windows).
        let out = "   pid 386(powerd): [0x1] 00:00:10 PreventUserIdleSystemSleep named: \"Wake Requests\"\n";
        assert_eq!(parse_assertions(out).len(), 1);
    }

    #[test]
    fn garbage_and_unknown_lines_are_tolerated() {
        assert!(parse_assertions("").is_empty());
        assert!(parse_assertions("Assertion status system-wide:\n   PreventUserIdleSystemSleep 1\nid=558 level=255\npid garbage\n").is_empty());
        // Timeout with nothing to attach to (fresh macOS format drift) → no panic.
        assert!(parse_assertions("\tTimeout will fire in 60 secs\n").is_empty());
    }

    #[test]
    fn profile_sleep_reads_the_right_line() {
        assert_eq!(parse_profile_sleep(REAL_PROFILE_AC), Some(0));
        // `displaysleep 10` / `disksleep 10` / `Sleep On Power Button 1` must
        // never win over the actual `sleep` line — battery fixture has sleep 1.
        let battery = " displaysleep         5\n disksleep            10\n Sleep On Power Button 1\n sleep                1\n";
        assert_eq!(parse_profile_sleep(battery), Some(1));
        assert_eq!(parse_profile_sleep("no sleep line here"), None);
    }

    #[test]
    fn aggregation_matches_the_real_capture() {
        let s = status_from(&parse_assertions(REAL_ASSERTIONS), parse_profile_sleep(REAL_PROFILE_AC));
        assert!(s.supported && s.prevented && s.indefinite && s.sleep_disabled);
        assert_eq!(s.max_timeout_secs, Some(274));
        assert_eq!(s.holders, vec!["caffeinate ×3", "sharingd", "bluetoothd"]);
    }

    #[test]
    fn aggregation_of_the_quiet_machine_is_all_clear() {
        let s = status_from(&[], Some(1));
        assert!(s.supported);
        assert!(!s.sleep_disabled && !s.prevented && !s.indefinite);
        assert_eq!(s.max_timeout_secs, None);
        assert!(s.holders.is_empty());
        // Unknown profile (pmset -g failed) degrades gracefully, not to "disabled".
        assert!(!status_from(&[], None).sleep_disabled);
    }

    #[test]
    fn timed_only_assertions_are_finite() {
        let a = vec![
            Assertion { process: "caffeinate".into(), timeout_secs: Some(120) },
            Assertion { process: "caffeinate".into(), timeout_secs: Some(300) },
        ];
        let s = status_from(&a, Some(1));
        assert!(s.prevented && !s.indefinite);
        assert_eq!(s.max_timeout_secs, Some(300));
        assert_eq!(s.holders, vec!["caffeinate ×2"]);
    }
}
