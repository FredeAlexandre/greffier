//! [`KeepAwake`]: an IOKit sleep assertion, held for the length of a recording or a processing.
//! The Mac no longer goes to sleep when left idle (`PreventUserIdleSystemSleep`); with `screen`,
//! the display doesn't turn off either (`PreventUserIdleDisplaySleep`, which also keeps the Mac
//! awake): that is needed during a recording, as ScreenCaptureKit captures nothing once the
//! display is off. Closing the lid still puts the Mac to sleep. The assertion is released when the
//! value is dropped (and by macOS if the app dies). It shows in `pmset -g assertions`, under the
//! given reason.

use objc2_core_foundation::CFString;

#[link(name = "IOKit", kind = "framework")]
unsafe extern "C" {
    fn IOPMAssertionCreateWithName(kind: &CFString, level: u32, name: &CFString, id: *mut u32) -> i32;
    fn IOPMAssertionRelease(id: u32) -> i32;
}

/// `kIOPMAssertionLevelOn`.
const ACTIVE: u32 = 255;

/// The Mac stays awake as long as this value lives.
#[derive(Debug)]
pub struct KeepAwake {
    id: u32,
    reason: String,
    screen: bool,
}

impl KeepAwake {
    /// Prevents the Mac's idle sleep, and the display's with `screen`; `reason` is the assertion's
    /// name ("Greffier enregistre une réunion"). `Err` if macOS refuses it.
    pub fn new(reason: &str, screen: bool) -> Result<KeepAwake, String> {
        let kind = CFString::from_str(if screen { "PreventUserIdleDisplaySleep" } else { "PreventUserIdleSystemSleep" });
        let name = CFString::from_str(reason);
        let mut id = 0u32;
        // SAFETY: two CFStrings valid for the call, `id` is a u32 of ours.
        let code = unsafe { IOPMAssertionCreateWithName(&kind, ACTIVE, &name, &mut id) };
        if code != 0 {
            return Err(format!("sleep assertion refused (IOKit {code:#x})"));
        }
        Ok(KeepAwake { id, reason: reason.into(), screen })
    }

    pub fn reason(&self) -> &str {
        &self.reason
    }

    /// The display stays on too.
    pub fn screen(&self) -> bool {
        self.screen
    }
}

impl Drop for KeepAwake {
    fn drop(&mut self) {
        // SAFETY: `id` comes from IOPMAssertionCreateWithName and is released only here.
        unsafe { IOPMAssertionRelease(self.id) };
    }
}
