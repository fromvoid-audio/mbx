//! VST3 view sizing and native presentation for the embedded editor.

#[cfg(target_os = "macos")]
mod fullscreen;

use std::{cell::Cell, ffi::c_void, ptr, rc::Rc};
use toybox::{
    gpui_gui::GpuiHostedGui,
    vst3::prelude::{Steinberg::*, *},
};

pub(super) const DEFAULT: (u32, u32) = (1080, 736);
pub(super) const MINIMUM: (u32, u32) = (920, 660);
pub(super) const MAXIMUM: (u32, u32) = (4096, 2400);
const SIZES: [(u32, u32); 3] = [MINIMUM, DEFAULT, (1440, 900)];

#[derive(Default)]
pub(super) struct Presentation {
    view: Cell<*mut IPlugView>,
    frame: Cell<*mut IPlugFrame>,
    attached: Cell<bool>,
    #[cfg(target_os = "macos")]
    fullscreen: std::cell::RefCell<Option<fullscreen::Session>>,
}

impl Presentation {
    pub fn can_request_size(&self) -> bool {
        self.attached.get() && !self.frame.get().is_null() && !self.is_fullscreen()
    }

    pub fn request_next_size(&self, current: (u32, u32), screen: (u32, u32)) {
        if !self.can_request_size() {
            return;
        }
        let frame = self.frame.get();
        let view = self.view.get();
        if view.is_null() {
            return;
        }
        let (width, height) = next_size(current, screen);
        let mut rect = ViewRect {
            left: 0,
            top: 0,
            right: width as i32,
            bottom: height as i32,
        };
        // The host owns its frame until setFrame replaces it. Keep both COM
        // objects alive across the synchronous callback, which can reenter onSize.
        unsafe {
            ((*(*frame).vtbl).base.addRef)(frame.cast());
            ((*(*view).vtbl).base.addRef)(view.cast());
            if let (Some(frame), Some(view)) = (ComPtr::from_raw(frame), ComPtr::from_raw(view)) {
                frame.resizeView(view.as_ptr(), &mut rect);
            }
        }
    }

    pub fn is_fullscreen(&self) -> bool {
        #[cfg(target_os = "macos")]
        {
            self.fullscreen
                .borrow()
                .as_ref()
                .is_some_and(fullscreen::Session::is_active)
        }
        #[cfg(not(target_os = "macos"))]
        {
            false
        }
    }

    #[cfg(target_os = "macos")]
    pub fn toggle_fullscreen(&self, window: &mut toybox::gpui::Window) {
        let windowed_size = if self.is_fullscreen() {
            let size = self
                .fullscreen
                .borrow()
                .as_ref()
                .map(fullscreen::Session::windowed_size);
            self.exit_fullscreen();
            size
        } else {
            if !self.attached.get() {
                return;
            }
            self.exit_fullscreen();
            self.fullscreen.replace(fullscreen::Session::enter(window));
            None
        };
        // AppKit changes the NSView frame. Synchronize the GPU drawable
        // independently of the still-windowed VST3 slot size.
        fullscreen::sync_drawable(window, windowed_size);
    }

    fn exit_fullscreen(&self) {
        #[cfg(target_os = "macos")]
        {
            let session = self.fullscreen.take();
            drop(session);
        }
    }
}

fn next_size(current: (u32, u32), screen: (u32, u32)) -> (u32, u32) {
    let available = SIZES
        .into_iter()
        .filter(|&(w, h)| w <= screen.0 && h <= screen.1);
    available
        .clone()
        .find(|&(w, h)| w >= current.0 && h >= current.1 && (w, h) != current)
        .or_else(|| available.into_iter().next())
        .unwrap_or(MINIMUM)
}

pub(super) fn dimensions(window: &toybox::gpui::Window) -> (u32, u32) {
    let size = window.viewport_size();
    (
        f32::from(size.width).round() as u32,
        f32::from(size.height).round() as u32,
    )
}

pub(super) fn screen_size(window: &toybox::gpui::Window) -> (u32, u32) {
    #[cfg(target_os = "macos")]
    if let Some(size) = fullscreen::available_size(window) {
        return size;
    }
    let _ = window;
    MAXIMUM
}

pub(super) struct EditorView {
    inner: HostedVst3View<GpuiHostedGui>,
    presentation: Rc<Presentation>,
}

impl EditorView {
    pub fn create(gui: GpuiHostedGui, presentation: Rc<Presentation>) -> *mut IPlugView {
        let inner = HostedVst3View::new(gui, DEFAULT.0, DEFAULT.1)
            .with_size_bounds(MINIMUM.0, MINIMUM.1, MAXIMUM.0, MAXIMUM.1)
            .preserve_aspect_ratio(false);
        let Some(view) = ComWrapper::new(Self {
            inner,
            presentation: Rc::clone(&presentation),
        })
        .to_com_ptr::<IPlugView>() else {
            return ptr::null_mut();
        };
        presentation.view.set(view.as_ptr());
        ComPtr::into_raw(view)
    }
}

impl Drop for EditorView {
    fn drop(&mut self) {
        self.presentation.attached.set(false);
        self.presentation.view.set(ptr::null_mut());
        self.presentation.frame.set(ptr::null_mut());
        self.presentation.exit_fullscreen();
    }
}

impl Class for EditorView {
    type Interfaces = (IPlugView,);
}

impl IPlugViewTrait for EditorView {
    unsafe fn isPlatformTypeSupported(&self, platform: FIDString) -> tresult {
        unsafe { self.inner.isPlatformTypeSupported(platform) }
    }

    unsafe fn attached(&self, parent: *mut c_void, platform: FIDString) -> tresult {
        let result = unsafe { self.inner.attached(parent, platform) };
        self.presentation.attached.set(result == kResultOk);
        result
    }

    unsafe fn removed(&self) -> tresult {
        self.presentation.attached.set(false);
        self.presentation.exit_fullscreen();
        unsafe { self.inner.removed() }
    }

    unsafe fn onWheel(&self, distance: f32) -> tresult {
        unsafe { self.inner.onWheel(distance) }
    }

    unsafe fn onKeyDown(&self, key: char16, code: int16, mods: int16) -> tresult {
        unsafe { self.inner.onKeyDown(key, code, mods) }
    }

    unsafe fn onKeyUp(&self, key: char16, code: int16, mods: int16) -> tresult {
        unsafe { self.inner.onKeyUp(key, code, mods) }
    }

    unsafe fn getSize(&self, bounds: *mut ViewRect) -> tresult {
        unsafe { self.inner.getSize(bounds) }
    }

    unsafe fn onSize(&self, bounds: *mut ViewRect) -> tresult {
        if bounds.is_null() {
            return kInvalidArgument;
        }
        // A DAW-initiated resize applies to the normal editor slot.
        self.presentation.exit_fullscreen();
        unsafe { self.inner.onSize(bounds) }
    }

    unsafe fn onFocus(&self, focused: TBool) -> tresult {
        unsafe { self.inner.onFocus(focused) }
    }

    unsafe fn setFrame(&self, frame: *mut IPlugFrame) -> tresult {
        self.presentation.frame.set(frame);
        kResultOk
    }

    unsafe fn canResize(&self) -> tresult {
        unsafe { self.inner.canResize() }
    }

    unsafe fn checkSizeConstraint(&self, bounds: *mut ViewRect) -> tresult {
        unsafe { self.inner.checkSizeConstraint(bounds) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn host_resize_uses_independent_axes_and_preserves_the_host_origin() {
        let gui = GpuiHostedGui::new(
            "MbxTestView",
            |_, _| unreachable!("test leaves the view detached"),
            DEFAULT.0,
            DEFAULT.1,
        )
        .with_size_contract(MINIMUM, DEFAULT, MAXIMUM);
        let view = EditorView::create(gui, Rc::new(Presentation::default()));
        let view = unsafe { ComPtr::from_raw(view) }.expect("IPlugView");
        let mut rect = ViewRect {
            left: 12,
            top: 19,
            right: 1223,
            bottom: 694,
        };
        assert_eq!(unsafe { view.canResize() }, kResultTrue);
        assert_eq!(unsafe { view.onSize(&mut rect) }, kResultOk);
        assert_eq!(
            [rect.left, rect.top, rect.right, rect.bottom],
            [12, 19, 1223, 694]
        );

        let mut tiny = ViewRect {
            left: 12,
            top: 19,
            right: 612,
            bottom: 419,
        };
        assert_eq!(unsafe { view.checkSizeConstraint(&mut tiny) }, kResultOk);
        assert_eq!(
            [tiny.right - tiny.left, tiny.bottom - tiny.top],
            [MINIMUM.0 as i32, MINIMUM.1 as i32]
        );
    }

    #[test]
    fn editor_size_presets_skip_sizes_that_do_not_fit() {
        assert_eq!(next_size(DEFAULT, MAXIMUM), (1440, 900));
        assert_eq!(next_size((1440, 900), MAXIMUM), MINIMUM);
        assert_eq!(next_size(DEFAULT, (1366, 810)), MINIMUM);
        assert_eq!(next_size(MINIMUM, (900, 550)), MINIMUM);
    }
}
