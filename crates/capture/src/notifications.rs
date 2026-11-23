//! The macOS permission to show notifications (« Bilan prêt »), through
//! `UNUserNotificationCenter`. Only an installed `Greffier.app` has one: callers check
//! `rune_notify::available()` first, otherwise the center throws.
//!
//! macOS answers asynchronously: [`state`] returns the last known answer and asks again in the
//! background, at most once a second, so the interface follows a change made in System Settings.

use std::sync::Mutex;
use std::sync::atomic::{AtomicU8, Ordering};
use std::time::{Duration, Instant};

use block2::RcBlock;
use objc2::rc::autoreleasepool;
use objc2::runtime::{AnyObject, Bool};
use objc2::{class, msg_send};

use greffier_core::adapters::PermissionState;

#[link(name = "UserNotifications", kind = "framework")]
unsafe extern "C" {}

/// UNAuthorizationOptionAlert: banners only, like the notifications Greffier posts.
const AUTH_ALERT: usize = 1 << 2;
/// UNAuthorizationStatus: notDetermined, denied, authorized, provisional, ephemeral.
const NOT_DETERMINED: u8 = 0;
const DENIED: u8 = 1;
/// Not asked to macOS yet.
const UNKNOWN: u8 = u8::MAX;

static STATUS: AtomicU8 = AtomicU8::new(UNKNOWN);
static LAST_CHECK: Mutex<Option<Instant>> = Mutex::new(None);

fn center() -> *mut AnyObject {
    // SAFETY: callers are in a bundled app (see the module doc), where this is the app's
    // singleton center.
    unsafe { msg_send![class!(UNUserNotificationCenter), currentNotificationCenter] }
}

/// Asks macOS for the current status, stored in [`STATUS`] when it answers.
fn refresh() {
    let done = RcBlock::new(|settings: *mut AnyObject| {
        if settings.is_null() {
            return;
        }
        // SAFETY: `settings` is a live UNNotificationSettings for the duration of the block.
        let status: isize = unsafe { msg_send![settings, authorizationStatus] };
        STATUS.store(status.clamp(0, 4) as u8, Ordering::Relaxed);
    });
    autoreleasepool(|_| unsafe {
        let _: () = msg_send![center(), getNotificationSettingsWithCompletionHandler: &*done];
    });
}

/// The notification permission, as macOS last gave it (granted while it hasn't answered yet,
/// so nothing flashes red at launch).
pub fn state() -> PermissionState {
    if let Ok(mut last) = LAST_CHECK.lock()
        && last.is_none_or(|t| t.elapsed() >= Duration::from_secs(1))
    {
        *last = Some(Instant::now());
        refresh();
    }
    match STATUS.load(Ordering::Relaxed) {
        NOT_DETERMINED => PermissionState::ToRequest,
        DENIED => PermissionState::Denied,
        _ => PermissionState::Granted,
    }
}

/// Asks for the permission if macOS has never asked; otherwise opens Greffier's page in System
/// Settings › Notifications, the only place where a refusal can be undone.
pub fn request() {
    if STATUS.load(Ordering::Relaxed) == NOT_DETERMINED {
        let done = RcBlock::new(|granted: Bool, _error: *mut AnyObject| {
            STATUS.store(if granted.as_bool() { 2 } else { DENIED }, Ordering::Relaxed);
        });
        autoreleasepool(|_| unsafe {
            let _: () = msg_send![center(), requestAuthorizationWithOptions: AUTH_ALERT, completionHandler: &*done];
        });
    } else {
        let id = autoreleasepool(|_| unsafe {
            let bundle: *mut AnyObject = msg_send![class!(NSBundle), mainBundle];
            let id: Option<objc2::rc::Retained<objc2_foundation::NSString>> = msg_send![bundle, bundleIdentifier];
            id.map(|s| s.to_string())
        });
        let url = format!("x-apple.systempreferences:com.apple.Notifications-Settings.extension?id={}", id.unwrap_or_default());
        let _ = std::process::Command::new("open").arg(url).spawn();
    }
}
