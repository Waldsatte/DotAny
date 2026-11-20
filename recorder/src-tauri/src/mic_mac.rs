//! The Mac asks the person before a program hears the microphone: the first recording shows the
//! system's question, with the reason in `Info.plist` (`NSMicrophoneUsageDescription`), and waits for the answer. Asked
//! here, before the microphone is opened: when the answer is no (now, or earlier in System Settings), the microphone
//! would only give silence, so the take isn't started and the page says it couldn't start, as it does for any
//! microphone that won't open. The question is AVFoundation's (`AVCaptureDevice`), the one every Mac program asks.

use block2::RcBlock;
use objc2::msg_send;
use objc2::runtime::{AnyClass, AnyObject, Bool};
use std::sync::Mutex;
use std::sync::mpsc::channel;
use std::time::Duration;

#[link(name = "AVFoundation", kind = "framework")]
unsafe extern "C" {
    static AVMediaTypeAudio: &'static AnyObject;
}

/// What the system has been told about this program and the microphone (`AVAuthorizationStatus`).
#[derive(Debug, PartialEq, Clone, Copy)]
pub enum Answer {
    NotAsked,
    /// A rule set for this Mac (a managed one) says no; the person can't change it.
    Restricted,
    Denied,
    Allowed,
}

pub fn answer() -> Answer {
    let Some(cls) = AnyClass::get(c"AVCaptureDevice") else { return Answer::Denied };
    let status: isize = unsafe { msg_send![cls, authorizationStatusForMediaType: AVMediaTypeAudio] };
    match status {
        0 => Answer::NotAsked,
        1 => Answer::Restricted,
        3 => Answer::Allowed,
        _ => Answer::Denied,
    }
}

/// Whether the program may hear the microphone: asks the person the first time, and waits for them.
pub fn allowed() -> bool {
    let now = answer();
    // a test says what the system answered, and may ask that nobody be asked: no question is left on the screen
    let test = std::env::var_os("DOTANY_TEST").is_some();
    if test {
        eprintln!("microphone: {now:?}");
    }
    // a test that must never hear the microphone, whatever the system says (the e2e on a Mac nobody sits at, where a
    // person allowing it earlier would otherwise let the test record the room): the answer is read, and taken as no
    if test && std::env::var_os("DOTANY_RECORDER_TEST_NEVER_HEAR").is_some() {
        return false;
    }
    match now {
        Answer::Allowed => true,
        Answer::Denied | Answer::Restricted => false,
        Answer::NotAsked if test && std::env::var_os("DOTANY_RECORDER_TEST_DONT_ASK").is_some() => false,
        Answer::NotAsked => {
            let Some(cls) = AnyClass::get(c"AVCaptureDevice") else { return false };
            let (tx, rx) = channel::<bool>();
            let tx = Mutex::new(Some(tx));
            let done = RcBlock::new(move |granted: Bool| {
                if let Some(tx) = tx.lock().ok().and_then(|mut t| t.take()) {
                    let _ = tx.send(granted.as_bool());
                }
            });
            let _: () =
                unsafe { msg_send![cls, requestAccessForMediaType: AVMediaTypeAudio, completionHandler: &*done] };
            // the person reads the question and answers: as long as they take
            rx.recv_timeout(Duration::from_secs(3600)).unwrap_or(false)
        }
    }
}
