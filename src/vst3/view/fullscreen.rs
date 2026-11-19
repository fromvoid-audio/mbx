use objc2::{rc::Retained, runtime::AnyObject, MainThreadMarker};
use objc2_app_kit::{
    NSApplication, NSFullScreenModeAllScreens, NSFullScreenModeApplicationPresentationOptions,
    NSView,
};
use objc2_foundation::{NSDictionary, NSNumber};
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use toybox::gpui::{px, size, Window};

pub(super) struct Session {
    view: Retained<NSView>,
    windowed_size: (f32, f32),
}

impl Session {
    pub fn enter(window: &Window) -> Option<Self> {
        let main_thread = MainThreadMarker::new()?;
        let view = native_view(window)?;
        let screen = view.window()?.screen()?;
        let bounds = view.bounds();
        let windowed_size = (bounds.size.width as f32, bounds.size.height as f32);
        let host_options = NSApplication::sharedApplication(main_thread).presentationOptions();
        let presentation = NSNumber::numberWithUnsignedInteger(host_options.bits());
        let all_screens = NSNumber::numberWithBool(false);
        // Preserve the DAW's application-level presentation and let AppKit
        // present only the plug-in's NSView on the editor's own display.
        let options = NSDictionary::from_slices(
            &[
                unsafe { NSFullScreenModeApplicationPresentationOptions },
                unsafe { NSFullScreenModeAllScreens },
            ],
            &[&*presentation as &AnyObject, &*all_screens as &AnyObject],
        );
        let entered = unsafe { view.enterFullScreenMode_withOptions(&screen, Some(&options)) };
        entered.then_some(Self {
            view,
            windowed_size,
        })
    }

    pub fn windowed_size(&self) -> (f32, f32) {
        self.windowed_size
    }

    pub fn is_active(&self) -> bool {
        self.view.isInFullScreenMode()
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        if self.is_active() {
            unsafe { self.view.exitFullScreenModeWithOptions(None) };
        }
    }
}

pub(super) fn sync_drawable(window: &mut Window, windowed_size: Option<(f32, f32)>) {
    if let Some(view) = native_view(window) {
        let bounds = view.bounds();
        let (width, height) =
            windowed_size.unwrap_or((bounds.size.width as f32, bounds.size.height as f32));
        window.resize(size(px(width), px(height)));
        if windowed_size.is_some() {
            if let Some(host) = view.window() {
                host.makeKeyAndOrderFront(None);
                host.makeFirstResponder(Some(&view));
            }
        }
        window.refresh();
    }
}

pub(super) fn available_size(window: &Window) -> Option<(u32, u32)> {
    let view = native_view(window)?;
    let frame = view.window()?.screen()?.visibleFrame();
    Some((
        frame.size.width.floor() as u32,
        (frame.size.height - 32.0).max(1.0).floor() as u32,
    ))
}

fn native_view(window: &Window) -> Option<Retained<NSView>> {
    let handle = HasWindowHandle::window_handle(window).ok()?;
    let RawWindowHandle::AppKit(handle) = handle.as_raw() else {
        return None;
    };
    // Toybox exposes its attached NSView through the borrowed GPUI window
    // handle. Retain it for the presentation so AppKit can restore that view.
    unsafe { Retained::retain(handle.ns_view.as_ptr().cast()) }
}
