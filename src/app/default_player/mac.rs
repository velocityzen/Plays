//! macOS: the type files of each extension have on this Mac, the app
//! LaunchServices opens that type with, and Plays made that app, a type
//! at a time. From macOS 12 that goes through NSWorkspace, which from 26.4
//! asks the user (Use or Keep) and calls back once they've answered;
//! before, through LaunchServices, which doesn't ask.

use std::ffi::c_void;
use std::path::Path;
use std::sync::Mutex;

use block::ConcreteBlock;
use core_foundation::base::TCFType;
use core_foundation::bundle::CFBundle;
use core_foundation::string::{CFString, CFStringRef};
use core_foundation::url::{CFURL, CFURLRef};
use file_types::{FORMATS, Format};
use futures::channel::oneshot;
use gpui::{App, AppContext as _, Task};
use objc::runtime::{BOOL, Class, NO, Object};
use objc::{class, msg_send, sel, sel_impl};

use super::{Made, Opener, Openers};

/// Plays' bundle identifier, as `resources/macos/Info.plist` has it.
pub const BUNDLE_ID: &str = "com.2dubs.plays";

/// Every role an app can have for a type: viewer, editor and so on.
const ALL_ROLES: u32 = 0xFFFF_FFFF;

/// The app that shows macOS's own prompts, such as whether Plays may open
/// a type.
const PROMPTS: &str = "com.apple.coreservices.uiagent";

#[link(name = "CoreServices", kind = "framework")]
unsafe extern "C" {
    fn UTTypeCreatePreferredIdentifierForTag(
        class: CFStringRef,
        tag: CFStringRef,
        conforming_to: CFStringRef,
    ) -> CFStringRef;
    fn LSCopyDefaultRoleHandlerForContentType(content_type: CFStringRef, role: u32) -> CFStringRef;
    fn LSCopyDefaultApplicationURLForContentType(
        content_type: CFStringRef,
        role: u32,
        error: *mut *const c_void,
    ) -> CFURLRef;
    fn LSSetDefaultRoleHandlerForContentType(
        content_type: CFStringRef,
        role: u32,
        handler: CFStringRef,
    ) -> i32;
}

/// The formats Plays offers itself for on macOS: those with a type.
fn offered() -> impl Iterator<Item = &'static Format> {
    FORMATS.iter().filter(|format| format.mac.is_some())
}

/// The type files ending in `.extension` have on this Mac, unless no app
/// declares one (macOS makes up a `dyn.` one then).
pub fn type_of(extension: &str) -> Option<String> {
    let class = CFString::from_static_string("public.filename-extension");
    let tag = CFString::new(extension);
    // SAFETY: both strings outlive the call, and a type returned is ours
    // to release (the Create rule), which wrapping it does.
    let id = unsafe {
        let id = UTTypeCreatePreferredIdentifierForTag(
            class.as_concrete_TypeRef(),
            tag.as_concrete_TypeRef(),
            std::ptr::null(),
        );
        (!id.is_null()).then(|| CFString::wrap_under_create_rule(id).to_string())
    }?;
    (!id.starts_with("dyn.")).then_some(id)
}

/// The types `format`'s files have on this Mac, one each, and its
/// extensions that have none.
fn types(format: &Format) -> (Vec<String>, Vec<&'static str>) {
    let mut types = Vec::new();
    let mut untyped = Vec::new();
    for &extension in format.extensions {
        match type_of(extension) {
            Some(id) if !types.contains(&id) => types.push(id),
            Some(_) => {}
            None => untyped.push(extension),
        }
    }
    (types, untyped)
}

/// The bundle identifier of the app that opens files of the type `id`.
fn handler(id: &str) -> Option<String> {
    let id = CFString::new(id);
    // SAFETY: the type outlives the call, and an identifier returned is
    // ours to release (the Copy rule), which wrapping it does.
    unsafe {
        let handler = LSCopyDefaultRoleHandlerForContentType(id.as_concrete_TypeRef(), ALL_ROLES);
        (!handler.is_null()).then(|| CFString::wrap_under_create_rule(handler).to_string())
    }
}

/// The name of the app that opens files of the type `id`: its bundle's,
/// without `.app`.
fn app_name(id: &str) -> Option<String> {
    let id = CFString::new(id);
    // SAFETY: the type outlives the call, which may leave no error (null
    // asked for); a URL returned is ours to release (the Copy rule), which
    // wrapping it does.
    let url = unsafe {
        let url = LSCopyDefaultApplicationURLForContentType(
            id.as_concrete_TypeRef(),
            ALL_ROLES,
            std::ptr::null_mut(),
        );
        (!url.is_null()).then(|| CFURL::wrap_under_create_rule(url))
    }?;
    let path = url.to_path()?;
    let name = Path::new(&path).file_stem()?.to_string_lossy().into_owned();
    Some(name)
}

/// The app that opens `format`'s files now: Plays if it does for each
/// type they have, or else the app that opens the first it doesn't.
pub fn opener(format: &'static Format) -> Opener {
    let (types, _) = types(format);
    let other = types
        .iter()
        .find(|id| handler(id).as_deref() != Some(BUNDLE_ID));
    Opener {
        format,
        app: match other {
            Some(id) => app_name(id),
            None if types.is_empty() => None,
            None => Some("Plays".to_owned()),
        },
        plays: !types.is_empty() && other.is_none(),
    }
}

/// Why Plays can't be made the default player: it isn't running as
/// Plays.app, whose bundle identifier LaunchServices names the app by.
pub fn cant() -> Option<String> {
    let bundle = CFBundle::main_bundle();
    let ours = bundle
        .info_dictionary()
        .find(CFString::from_static_string("CFBundleIdentifier"))
        .and_then(|id| id.downcast::<CFString>())
        .is_some_and(|id| id == BUNDLE_ID);
    (!ours).then(|| {
        "only Plays.app can be the default player, and this Plays isn't it: \
         script/bundle-macos makes it"
            .to_owned()
    })
}

/// Each format Plays opens, and its app now. LaunchServices answers from
/// any thread.
pub fn read(cx: &mut App) -> Task<Openers> {
    cx.background_spawn(async { Ok(offered().map(opener).collect()) })
}

/// Make Plays open `formats`, a type at a time, waiting for the user to
/// answer macOS about each before asking about the next; then what each
/// opens in now says what came of it.
pub fn make(formats: Vec<&'static Format>, cx: &mut App) -> Task<Made> {
    if cfg!(test) {
        panic!("a test would change the apps this Mac opens files with");
    }
    cx.spawn(async move |cx| {
        let mut made = Made::default();
        for format in formats {
            let (types, untyped) = types(format);
            if types.is_empty() {
                let why = format!("this Mac has no type for .{} files", untyped.join(", ."));
                made.not_set.push((format, why));
                continue;
            }
            for id in types
                .iter()
                .filter(|id| handler(id).as_deref() != Some(BUNDLE_ID))
            {
                if let Ok(Some(answered)) = cx.update(|_| ask(id)) {
                    answered.await.ok();
                }
            }
            let now = opener(format);
            if now.plays {
                made.set.push(format);
            } else {
                let why = match now.app {
                    Some(app) => format!("{app} still opens them"),
                    None => "macOS didn't change it".to_owned(),
                };
                made.not_set.push((format, why));
            }
        }
        made
    })
}

/// Ask macOS for Plays, this bundle, to open files of the type `id`. What
/// it returns hears once macOS has done, from 26.4 once the user answered;
/// nothing when it's done already, as it is before macOS 12.
#[allow(unexpected_cfgs)]
fn ask(id: &str) -> Option<oneshot::Receiver<()>> {
    let id = CFString::new(id);
    let set = sel!(setDefaultApplicationAtURL:toOpenContentType:completionHandler:);
    // SAFETY: AppKit calls on the main thread, where `make` runs, each
    // object checked before use; `id` outlives the calls, and the block is
    // copied by NSWorkspace, which keeps it until it calls it.
    unsafe {
        let workspace: *mut Object = msg_send![class!(NSWorkspace), sharedWorkspace];
        let new_api: BOOL = msg_send![workspace, respondsToSelector: set];
        let types = Class::get("UTType");
        let Some(types) = types.filter(|_| new_api != NO) else {
            let plays = CFString::from_static_string(BUNDLE_ID);
            LSSetDefaultRoleHandlerForContentType(
                id.as_concrete_TypeRef(),
                ALL_ROLES,
                plays.as_concrete_TypeRef(),
            );
            return None;
        };
        let content_type: *mut Object =
            msg_send![types, typeWithIdentifier: id.as_concrete_TypeRef()];
        let bundle: *mut Object = msg_send![class!(NSBundle), mainBundle];
        let app: *mut Object = msg_send![bundle, bundleURL];
        if content_type.is_null() || app.is_null() {
            return None;
        }
        let (done, answered) = oneshot::channel();
        let done = Mutex::new(Some(done));
        let callback = ConcreteBlock::new(move |_error: *mut Object| {
            if let Some(done) = done.lock().ok().and_then(|mut done| done.take()) {
                done.send(()).ok();
            }
        })
        .copy();
        let () = msg_send![workspace, setDefaultApplicationAtURL: app
            toOpenContentType: content_type
            completionHandler: &*callback];
        bring_prompt_forward();
        Some(answered)
    }
}

/// Bring macOS's prompt to the front, which it may put behind Plays'
/// windows, Settings floating over others with a player kept on top.
#[allow(unexpected_cfgs)]
fn bring_prompt_forward() {
    /// `NSApplicationActivateAllWindows`.
    const ALL_WINDOWS: u64 = 1;
    let prompts = CFString::from_static_string(PROMPTS);
    // SAFETY: AppKit calls on the main thread, the app checked before use.
    unsafe {
        let apps: *mut Object = msg_send![
            class!(NSRunningApplication),
            runningApplicationsWithBundleIdentifier: prompts.as_concrete_TypeRef()
        ];
        let app: *mut Object = msg_send![apps, firstObject];
        if !app.is_null() {
            let _: BOOL = msg_send![app, activateWithOptions: ALL_WINDOWS];
        }
    }
}
