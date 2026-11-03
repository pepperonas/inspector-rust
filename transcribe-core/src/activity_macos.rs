//! Keep macOS from napping the helper while it records (App Nap would
//! throttle timers and the capture threads).

use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::{class, msg_send};
use objc2_foundation::NSString;

/// `NSActivityUserInitiated`: no App Nap, no idle system sleep.
const USER_INITIATED: u64 = 0x00FF_FFFF | (1 << 20);

pub struct Activity(Retained<AnyObject>);

// SAFETY: the activity token is an immutable NSObject; `endActivity:` is thread-safe.
unsafe impl Send for Activity {}

impl Activity {
    pub fn begin(reason: &str) -> Option<Activity> {
        let reason = NSString::from_str(reason);
        // SAFETY: documented NSProcessInfo API; the token is retained by us.
        unsafe {
            let info: *mut AnyObject = msg_send![class!(NSProcessInfo), processInfo];
            let token: *mut AnyObject = msg_send![info, beginActivityWithOptions: USER_INITIATED, reason: &*reason];
            Retained::retain(token).map(Activity)
        }
    }
}

impl Drop for Activity {
    fn drop(&mut self) {
        // SAFETY: ending the activity we began.
        unsafe {
            let info: *mut AnyObject = msg_send![class!(NSProcessInfo), processInfo];
            let _: () = msg_send![info, endActivity: &*self.0];
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn begin_and_end() {
        let a = super::Activity::begin("test");
        assert!(a.is_some());
    }
}
