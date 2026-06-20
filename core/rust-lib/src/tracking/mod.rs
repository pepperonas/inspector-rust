//! Timesheet / time-tracking feature.
//!
//! Delivery is incremental (see `docs/timesheet.md`); this module currently
//! provides the persistence layer (`db`). The tracker core (focus loop, idle
//! detection), the browser bridge, the Claude watcher and the IPC/UI land in
//! subsequent steps.
//
// The persistence API is exercised by its own unit tests but not yet by the
// crate (the IPC commands + tracker core that call it land in the next delivery
// steps), so allow dead_code for now — it's removed as the consumers are wired.
#![allow(dead_code)]

pub mod db;
