use super::{
    finish, plot, with_group_edit, EditIds, Editor, Handler, ACCENT, HAIR, MUTED, PAPER, TEXT,
};
use crate::parameters::{self, layout::Layout, BandParam as P, Values, BANDS, PHASE};
use crate::vst3::Parameters;
use std::{
    cell::RefCell,
    time::{Duration, Instant},
};
use toybox::{
    gpui::{
        div, point, px, rgb, size, AnyElement, Bounds, Context, InteractiveElement, IntoElement,
        MouseButton, ParentElement, Pixels, Styled,
    },
    vst3::prelude::IComponentHandlerTrait,
};

const HOLD: Duration = Duration::from_millis(350);
const STRIP_HEIGHT: f32 = 24.0;
const STRIP_WIDTH: f32 = 414.0;

pub struct Press {
    before: Vec<(usize, f64)>,
    started: Instant,
}

pub type PressSlot = RefCell<Option<Press>>;

fn listening_param(id: usize) -> Option<P> {
    [P::Solo, P::Mute, P::Audition]
        .into_iter()
        .find(|field| (0..BANDS).any(|band| field.id(band) == id))
}

fn toggle(values: &Values, id: usize, exclusive: bool) -> Vec<(usize, f64)> {
    let next = 1.0 - values[id];
    let mut edits = vec![(id, next)];
    if let Some(field) = listening_param(id).filter(|_| exclusive) {
        edits.extend(
            (0..BANDS)
                .map(|band| field.id(band))
                .filter(|&target| target != id && values[target] > 0.5)
                .map(|target| (target, 0.0)),
        );
    }
    edits
}

pub fn restore_press(press: &PressSlot, params: &Parameters, handler: &Handler, cancelled: bool) {
    let Some(press) = press.take() else { return };
    if !cancelled && press.started.elapsed() < HOLD {
        return;
    }
    with_group_edit(handler, press.before.len() > 1, || {
        for (id, value) in press.before {
            params.set(id, value);
            let handler = handler.borrow().clone();
            if let Some(handler) = handler {
                unsafe { handler.performEdit(id as u32, value) };
            }
        }
    });
}

pub fn cancel_pointer(press: &PressSlot, params: &Parameters, handler: &Handler, edits: &EditIds) {
    restore_press(press, params, handler, true);
    finish(handler, edits);
}

impl Editor {
    pub fn button_toggle(&self, id: usize, exclusive: bool) {
        if let Some(band) = (0..BANDS).find(|&band| id == P::Enabled.id(band)) {
            self.edit_many([(P::Present.id(band), 1.0), (id, 1.0 - self.params.get(id))]);
        } else {
            self.edit_many(toggle(&self.params.values(), id, exclusive));
        }
    }

    pub fn button_press(&mut self, id: usize, exclusive: bool) {
        if listening_param(id).is_none() {
            self.button_toggle(id, exclusive);
            return;
        }
        restore_press(&self.press, &self.params, &self.handler, true);
        self.finish_edit();
        self.drag = None;
        self.entry = None;
        let current = self.params.values();
        let edits = toggle(&current, id, exclusive);
        self.press.replace(Some(Press {
            before: edits.iter().map(|&(id, _)| (id, current[id])).collect(),
            started: Instant::now(),
        }));
        with_group_edit(&self.handler, edits.len() > 1, || {
            self.begin(edits.iter().map(|&(id, _)| id));
            for (id, value) in edits {
                self.change(id, value);
            }
        });
    }

    pub fn bands_button(&self, cx: &mut Context<Self>) -> AnyElement {
        let focus = self.bands_focus.clone();
        div()
            .id("bands-panel-toggle")
            .track_focus(&focus)
            .tab_stop(true)
            .border_1()
            .border_color(rgb(if self.bands_open { MUTED } else { HAIR }))
            .focus(|style| style.border_color(rgb(ACCENT)))
            .rounded(px(4.0))
            .bg(rgb(PAPER))
            .hover(|style| style.bg(rgb(0x393939)))
            .cursor_pointer()
            .text_size(px(11.0))
            .px(px(8.0))
            .py(px(3.0))
            .child(if self.bands_open {
                "Bands ▾"
            } else {
                "Bands ▸"
            })
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, _, window, cx| {
                    window.focus(&focus, cx);
                    this.bands_open = !this.bands_open;
                    cx.notify();
                }),
            )
            .on_key_down(
                cx.listener(|this, event: &toybox::gpui::KeyDownEvent, _, cx| {
                    if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                        this.bands_open = !this.bands_open;
                        cx.stop_propagation();
                        cx.notify();
                    }
                }),
            )
            .into_any_element()
    }

    pub fn phase_selector(&self, cx: &mut Context<Self>) -> AnyElement {
        let linear = self.params.get(PHASE) > 0.5;
        let choice = |option: bool, cx: &mut Context<Self>| {
            let focus = if option {
                &self.phase_linear_focus
            } else {
                &self.focus[PHASE]
            }
            .clone();
            div()
                .id(("phase-choice", usize::from(option)))
                .track_focus(&focus)
                .tab_stop(true)
                .border_1()
                .border_color(rgb(if option == linear { ACCENT } else { HAIR }))
                .focus(|style| style.border_color(rgb(TEXT)))
                .hover(|style| style.bg(rgb(0x393939)))
                .rounded(px(3.0))
                .bg(rgb(if option == linear { 0x3b2827 } else { PAPER }))
                .cursor_pointer()
                .px(px(6.0))
                .py(px(3.0))
                .text_size(px(11.0))
                .child(if option { "Linear" } else { "Minimum" })
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |this, _, window, cx| {
                        window.focus(&focus, cx);
                        this.edit(PHASE, f64::from(option));
                        cx.notify();
                    }),
                )
                .on_key_down(cx.listener(
                    move |this, event: &toybox::gpui::KeyDownEvent, window, cx| {
                        let target = match event.keystroke.key.as_str() {
                            "enter" | "space" => option,
                            "left" => false,
                            "right" => true,
                            _ => return,
                        };
                        let focus = if target {
                            &this.phase_linear_focus
                        } else {
                            &this.focus[PHASE]
                        };
                        window.focus(focus, cx);
                        this.edit(PHASE, f64::from(target));
                        cx.stop_propagation();
                        cx.notify();
                    },
                ))
        };
        div()
            .flex()
            .flex_col()
            .gap(px(3.0))
            .child(
                div()
                    .text_size(px(11.0))
                    .text_color(rgb(MUTED))
                    .child("Phase"),
            )
            .child(
                div()
                    .flex()
                    .gap(px(4.0))
                    .child(choice(false, cx))
                    .child(choice(true, cx)),
            )
            .into_any_element()
    }

    pub fn graph_controls(
        &self,
        band: usize,
        values: &Values,
        view: plot::Viewport,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let chart = self.plot.get()?;
        let anchor = hover_strip(chart, view, values, band)?;
        let range = plot::range_text(values, band);
        Some(
            div()
                .absolute()
                .left(anchor.left() - chart.left())
                .top(anchor.top() - chart.top())
                .w(anchor.size.width)
                .h(anchor.size.height)
                .flex()
                .items_center()
                .justify_between()
                .gap(px(3.0))
                .px(px(6.0))
                .rounded(px(4.0))
                .border_1()
                .border_color(rgb(HAIR))
                .bg(rgb(PAPER))
                .child(
                    div()
                        .text_size(px(10.0))
                        .px(px(3.0))
                        .child(format!("Band {}  ·  {range}", band + 1)),
                )
                .child(self.switch_button(P::Solo.id(band), "Solo", ACCENT, Some(0), cx))
                .child(self.switch_button(P::Mute.id(band), "Mute", ACCENT, Some(1), cx))
                .child(self.switch_button(P::Audition.id(band), "Key", ACCENT, Some(2), cx))
                .child(self.slope_chip(P::LowSlope.id(band), "L", 3, cx))
                .child(self.slope_chip(P::HighSlope.id(band), "H", 4, cx))
                .into_any_element(),
        )
    }

    fn slope_chip(
        &self,
        id: usize,
        prefix: &'static str,
        index: usize,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let spec = parameters::spec(id);
        let text = format!("{prefix} {}", spec.display(self.params.get(id)));
        let focus = self.graph_focus[index].clone();
        div()
            .id(("graph-slope", index))
            .track_focus(&focus)
            .tab_stop(true)
            .rounded(px(3.0))
            .border_1()
            .border_color(rgb(HAIR))
            .focus(|style| style.border_color(rgb(ACCENT)))
            .hover(|style| style.bg(rgb(0x373737)))
            .px(px(4.0))
            .text_size(px(9.0))
            .text_color(rgb(TEXT))
            .cursor_pointer()
            .child(text)
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(
                    move |this, event: &toybox::gpui::MouseDownEvent, window, cx| {
                        window.focus(&focus, cx);
                        this.control_step(id, if event.modifiers.shift { -1 } else { 1 }, false);
                        cx.stop_propagation();
                        cx.notify();
                    },
                ),
            )
            .on_key_down(
                cx.listener(move |this, event: &toybox::gpui::KeyDownEvent, _, cx| {
                    let step = match event.keystroke.key.as_str() {
                        "left" | "down" => -1,
                        "enter" | "space" | "up" | "right" => 1,
                        _ => return,
                    };
                    this.control_step(id, step, false);
                    cx.stop_propagation();
                    cx.notify();
                }),
            )
            .into_any_element()
    }
}

pub fn hover_strip(
    chart: Bounds<Pixels>,
    view: plot::Viewport,
    values: &Values,
    band: usize,
) -> Option<Bounds<Pixels>> {
    let edges = Layout::new(values).edges[band];
    if !view.contains((edges[0] + edges[1]) * 0.5) {
        return None;
    }
    let (center, limit) = plot::band_anchor(chart, view, values, band);
    let width = px(STRIP_WIDTH);
    let height = px(STRIP_HEIGHT);
    let left =
        (center.x - width * 0.5).clamp(chart.left() + px(16.0), chart.right() - width - px(16.0));
    let top = if center.y >= chart.top() + height + px(40.0) {
        center.y - height - px(12.0)
    } else {
        (limit.max(center.y) + px(12.0)).min(chart.top() + px(plot::ruler(chart)) - height)
    };
    Some(Bounds::new(point(left, top), size(width, height)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exclusive_listening_toggles_only_other_slots_of_the_same_kind() {
        let mut values = parameters::defaults();
        values[P::Solo.id(0)] = 1.0;
        values[P::Mute.id(0)] = 1.0;
        let edits = toggle(&values, P::Solo.id(1), true);
        assert_eq!(edits, vec![(P::Solo.id(1), 1.0), (P::Solo.id(0), 0.0)]);
    }

    #[test]
    fn a_held_or_cancelled_switch_restores_its_previous_value() {
        let params = Parameters::new();
        let handler = Handler::default();
        let id = P::Solo.id(1);
        let press = PressSlot::default();
        for (started, cancelled) in [(Instant::now() - HOLD, false), (Instant::now(), true)] {
            params.set(id, 1.0);
            press.replace(Some(Press {
                before: vec![(id, 0.0)],
                started,
            }));
            restore_press(&press, &params, &handler, cancelled);
            assert_eq!(params.get(id), 0.0);
        }
    }
}
