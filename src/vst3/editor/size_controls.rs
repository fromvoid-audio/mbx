use super::{Editor, ACCENT, HAIR, PAPER};
use crate::vst3::view;
use std::time::Duration;
use toybox::gpui::{
    div, px, rgb, AnyElement, Context, InteractiveElement, IntoElement, KeyDownEvent, MouseButton,
    ParentElement, Styled, Window,
};

#[derive(Clone, Copy)]
enum Action {
    Size,
    #[cfg(target_os = "macos")]
    Fullscreen,
}

impl Editor {
    pub(super) fn size_button(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        self.presentation
            .can_request_size()
            .then(|| self.size_control(Action::Size, "Size", cx))
    }

    pub(super) fn fullscreen_button(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        #[cfg(target_os = "macos")]
        {
            let label = if self.presentation.is_fullscreen() {
                "Exit full screen"
            } else {
                "Full screen"
            };
            Some(self.size_control(Action::Fullscreen, label, cx))
        }
        #[cfg(not(target_os = "macos"))]
        {
            let _ = cx;
            None
        }
    }

    #[cfg(target_os = "macos")]
    pub(super) fn toggle_screen(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.size_action(Action::Fullscreen, window, cx);
    }

    fn size_action(&mut self, action: Action, window: &mut Window, cx: &mut Context<Self>) {
        self.release(cx);
        self.entry = None;
        cx.spawn_in(window, async move |editor, cx| {
            // resizeView may call onSize synchronously. Do not reenter the
            // VST3 GUI while a host keyboard callback still owns its lock.
            cx.background_executor()
                .timer(Duration::from_millis(1))
                .await;
            match action {
                Action::Size => {
                    let request = editor.update_in(cx, |editor, window, _| {
                        (
                            editor.presentation.clone(),
                            view::dimensions(window),
                            view::screen_size(window),
                        )
                    });
                    if let Ok((presentation, current, screen)) = request {
                        presentation.request_next_size(current, screen);
                    }
                }
                #[cfg(target_os = "macos")]
                Action::Fullscreen => {
                    let _ = editor.update_in(cx, |editor, window, cx| {
                        editor.presentation.toggle_fullscreen(window);
                        cx.notify();
                    });
                }
            }
        })
        .detach();
    }

    fn size_control(
        &self,
        action: Action,
        label: &'static str,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let focus = self.size_focus[action as usize].clone();
        div()
            .id(("editor-size", action as usize))
            .track_focus(&focus)
            .tab_stop(true)
            .flex_none()
            .rounded(px(4.0))
            .border_1()
            .border_color(rgb(HAIR))
            .focus(|style| style.border_color(rgb(ACCENT)))
            .bg(rgb(PAPER))
            .hover(|style| style.bg(rgb(0x393939)))
            .px(px(8.0))
            .py(px(3.0))
            .cursor_pointer()
            .text_size(px(11.0))
            .child(label)
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |editor, _, window, cx| {
                    window.focus(&focus, cx);
                    editor.size_action(action, window, cx);
                }),
            )
            .on_key_down(
                cx.listener(move |editor, event: &KeyDownEvent, window, cx| {
                    if matches!(event.keystroke.key.as_str(), "space" | "enter") {
                        editor.size_action(action, window, cx);
                        cx.stop_propagation();
                    }
                }),
            )
            .into_any_element()
    }
}
