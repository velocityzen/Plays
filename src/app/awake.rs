//! Keeping the screen awake while a video plays, and the computer while only
//! sound does. Once nothing plays, the system sleeps as it would.

use std::time::Duration;

use gpui::App;

use crate::ui::workspace::Workspace;

/// How often what plays is looked at.
const POLL: Duration = Duration::from_secs(1);

/// What playing keeps awake.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug, Default)]
pub enum Awake {
    #[default]
    Nothing,
    /// The computer, but the screen may sleep: only sound plays.
    Computer,
    /// The screen, and so the computer: a video plays.
    Screen,
}

/// Look at what plays in every window, and keep awake what it needs.
pub fn watch(cx: &mut App) {
    cx.spawn(async move |cx| {
        let mut awake = Awake::Nothing;
        let mut hold = None;
        loop {
            cx.background_executor().timer(POLL).await;
            let Ok(want) = cx.update(playing) else {
                return;
            };
            if want == awake {
                continue;
            }
            log::debug!("keeping awake: {want:?}");
            if let Some(old) = hold.take() {
                platform::release(old).await;
            }
            awake = want;
            if want != Awake::Nothing {
                hold = platform::hold(want).await;
            }
        }
    })
    .detach();
}

/// What the players in every window keep awake.
fn playing(cx: &mut App) -> Awake {
    cx.windows()
        .into_iter()
        .filter_map(|window| window.downcast::<Workspace>())
        .filter_map(|window| window.read(cx).ok().map(|w| w.keeps_awake(cx)))
        .max()
        .unwrap_or_default()
}

/// Power assertions, which the system drops itself when the app quits.
#[cfg(target_os = "macos")]
mod platform {
    use core_foundation::base::TCFType;
    use core_foundation::string::{CFString, CFStringRef};

    use super::Awake;

    const ASSERTION_LEVEL_ON: u32 = 255;

    #[link(name = "IOKit", kind = "framework")]
    unsafe extern "C" {
        fn IOPMAssertionCreateWithName(
            kind: CFStringRef,
            level: u32,
            name: CFStringRef,
            id: *mut u32,
        ) -> i32;
        fn IOPMAssertionRelease(id: u32) -> i32;
    }

    pub struct Hold(u32);

    pub async fn hold(awake: Awake) -> Option<Hold> {
        let (kind, name) = match awake {
            Awake::Nothing => return None,
            Awake::Computer => ("PreventUserIdleSystemSleep", "Playing sound"),
            Awake::Screen => ("PreventUserIdleDisplaySleep", "Playing a video"),
        };
        let (kind, name) = (CFString::new(kind), CFString::new(name));
        let mut id = 0;
        // SAFETY: both strings live through the call, and `id` is written
        // only on success.
        let status = unsafe {
            IOPMAssertionCreateWithName(
                kind.as_concrete_TypeRef(),
                ASSERTION_LEVEL_ON,
                name.as_concrete_TypeRef(),
                &mut id,
            )
        };
        if status != 0 {
            log::warn!("could not keep awake: IOKit error {status:#x}");
            return None;
        }
        Some(Hold(id))
    }

    pub async fn release(hold: Hold) {
        // SAFETY: the id came from `IOPMAssertionCreateWithName`, once.
        unsafe { IOPMAssertionRelease(hold.0) };
    }
}

/// The desktop portal's inhibitors, which it drops itself when the app
/// quits.
#[cfg(target_os = "linux")]
mod platform {
    use ashpd::desktop::Request;
    use ashpd::desktop::inhibit::{InhibitFlags, InhibitProxy};

    use super::Awake;

    pub struct Hold(Request<()>);

    pub async fn hold(awake: Awake) -> Option<Hold> {
        let (flags, reason) = match awake {
            Awake::Nothing => return None,
            Awake::Computer => (InhibitFlags::Suspend, "Playing sound"),
            Awake::Screen => (InhibitFlags::Idle, "Playing a video"),
        };
        let request = async {
            InhibitProxy::new()
                .await?
                .inhibit(None, flags.into(), reason)
                .await
        };
        match request.await {
            Ok(request) => Some(Hold(request)),
            Err(err) => {
                log::warn!("could not keep awake: {err}");
                None
            }
        }
    }

    pub async fn release(hold: Hold) {
        if let Err(err) = hold.0.close().await {
            log::debug!("could not stop keeping awake: {err}");
        }
    }
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
mod platform {
    use super::Awake;

    pub struct Hold;

    pub async fn hold(_awake: Awake) -> Option<Hold> {
        None
    }

    pub async fn release(_hold: Hold) {}
}
