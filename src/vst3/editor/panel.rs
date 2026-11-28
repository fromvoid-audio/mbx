use super::{
    icon::Icon,
    plot,
    selection::{members, ModeAxis},
    Command, Editor, ValueStyle, ACCENT, BG, HAIR, MUTED, PAPER, TEXT,
};
use crate::parameters::{detector::Source, BandParam as P, Values};
use std::sync::atomic::Ordering::Relaxed;
use toybox::gpui::{
    div, point, px, rgb, size, AccessibleAction, AnyElement, Bounds, Context, Div,
    InteractiveElement, IntoElement, MouseButton, ParentElement, Pixels, Point, Role,
    StatefulInteractiveElement, Styled, Toggled,
};

const WIDTH: f32 = 688.0;
const MARGIN: f32 = 38.0;
// Clear the frequency labels and the ruler's drag area above its line.
const BOTTOM: f32 = plot::FOOT + 16.0;
const PADDING: f32 = 9.0;
const HEADER: f32 = 26.0;
const MAIN_ROW: f32 = 90.0;
const SMALL_ROW: f32 = 34.0;
const ROW_GAP: f32 = 4.0;
const DYNAMICS_HEIGHT: f32 = MAIN_ROW + ROW_GAP + SMALL_ROW;
const CROSSOVER_WIDTH: f32 = 161.0;
const DIVIDER: u32 = 0x333333;
const PRIMARY: &[P] = &[P::Threshold, P::Range, P::Attack, P::Release, P::Makeup];
const DYNAMICS: &[P] = &[P::Ratio, P::Knee, P::Lookahead];
const SWITCHES: [(P, Icon, &str); 4] = [
    (P::Enabled, Icon::Power, "Band enabled"),
    (P::Solo, Icon::Solo, "Solo band"),
    (P::Mute, Icon::Mute, "Mute band"),
    (P::Audition, Icon::Audition, "Audition key"),
];
const DETECTOR: &[P] = &[
    P::Key,
    P::KeyFilter,
    P::KeyLow,
    P::KeyHigh,
    P::Route,
    P::Link,
];

#[derive(Clone, Copy)]
pub(super) struct Placement {
    left: Pixels,
    width: Pixels,
    height: Pixels,
}

impl Placement {
    fn at_bottom(chart: Bounds<Pixels>, band_x: Pixels, detector: bool) -> Self {
        let margin = ((chart.size.width - px(WIDTH)) * 0.5).clamp(px(12.0), px(MARGIN));
        let width = px(WIDTH).min(chart.size.width - margin * 2.0);
        let height = px((PADDING + 1.0) * 2.0
            + HEADER
            + ROW_GAP
            + DYNAMICS_HEIGHT
            + if detector { SMALL_ROW + ROW_GAP } else { 0.0 });
        let left = (band_x - chart.left() - width / 2.0)
            .clamp(margin, (chart.size.width - margin - width).max(margin));
        Self {
            left,
            width,
            height,
        }
    }

    pub(super) fn bounds(self, chart: Bounds<Pixels>) -> Bounds<Pixels> {
        Bounds::new(
            point(
                chart.left() + self.left,
                chart.bottom() - px(BOTTOM) - self.height,
            ),
            size(self.width, self.height),
        )
    }

    pub fn contains(self, chart: Bounds<Pixels>, point: Point<Pixels>) -> bool {
        self.bounds(chart).contains(&point)
    }
}

impl Editor {
    pub(super) fn band_inspector(
        &mut self,
        band: usize,
        values: &Values,
        view: plot::Viewport,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let chart = self.plot.get()?;
        let (node, _) = plot::band_anchor(chart, view, values, band);
        let placement = Placement::at_bottom(chart, node.x, self.detector_open);
        self.panel = Some(placement);
        let Placement {
            left,
            width,
            height,
        } = placement;
        let selected = self.selection.count_ones();
        let title = if selected > 1 {
            format!("{selected} bands")
        } else {
            format!("Band {}", band + 1)
        };
        let external = Source::read(values, band) == Source::External;
        let inactive = external && self.meters.sidechain.load(Relaxed) == 0;
        let [power, solo, mute, audition] = std::array::from_fn(|index| {
            let (field, icon, label) = SWITCHES[index];
            let id = field.id(band);
            let active = values[id] > 0.5;
            let editor = cx.entity().downgrade();
            let button = icon
                .button(label, active)
                .aria_toggled(if active {
                    Toggled::True
                } else {
                    Toggled::False
                })
                .on_a11y_action(AccessibleAction::Click, move |_, _, cx| {
                    editor
                        .update(cx, |this, cx| {
                            this.button_toggle(id, false);
                            cx.notify();
                        })
                        .ok();
                });
            self.switch_control(id, self.inspector_switch_focus[index].clone(), button, cx)
        });
        let panel =
            div()
                .id("band-inspector")
                .absolute()
                .left(left)
                .bottom(px(BOTTOM))
                .w(width)
                .h(height)
                .p(px(PADDING))
                .gap(px(ROW_GAP))
                .flex()
                .flex_col()
                .rounded(px(8.0))
                .border_1()
                .border_color(rgb(0x575757))
                .bg(rgb(BG))
                .shadow_md()
                .on_any_mouse_down(|_, _, cx| cx.stop_propagation())
                .on_scroll_wheel(|_, _, cx| cx.stop_propagation())
                .child(
                    div()
                        .h(px(HEADER))
                        .flex_none()
                        .flex()
                        .items_center()
                        .gap(px(5.0))
                        .child(power)
                        .child(div().text_size(px(13.0)).child(title))
                        .children((selected == 1 || inactive).then(|| {
                            div()
                                .text_size(px(10.0))
                                .text_color(rgb(MUTED))
                                .child(if inactive {
                                    "Sidechain inactive".to_owned()
                                } else {
                                    plot::range_text(values, band)
                                })
                        }))
                        .child(div().flex_1())
                        .children([solo, mute, audition])
                        .child(self.detector_toggle(cx))
                        .child(self.chart_button(Command::Remove, cx))
                        .child(self.chart_button(Command::Deselect, cx)),
                )
                .child(
                    div()
                        .h(px(DYNAMICS_HEIGHT))
                        .flex_none()
                        .flex()
                        .gap(px(9.0))
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .flex()
                                .flex_col()
                                .gap(px(ROW_GAP))
                                .child(div().h(px(MAIN_ROW)).flex_none().flex().children(
                                    PRIMARY.iter().map(|field| {
                                        self.value(field.id(band), ValueStyle::Knob, cx)
                                    }),
                                ))
                                .child(
                                    div()
                                        .h(px(SMALL_ROW))
                                        .flex_none()
                                        .flex()
                                        .gap(px(2.0))
                                        .border_t_1()
                                        .border_color(rgb(DIVIDER))
                                        .child(self.mode_selectors(values, cx))
                                        .children(DYNAMICS.iter().map(|field| {
                                            self.value(field.id(band), ValueStyle::Compact, cx)
                                        })),
                                ),
                        )
                        .child(
                            div()
                                .w(px(CROSSOVER_WIDTH))
                                .flex_none()
                                .border_l_1()
                                .border_color(rgb(DIVIDER))
                                .pl(px(8.0))
                                .flex()
                                .flex_col()
                                .justify_center()
                                .child(
                                    div()
                                        .pl(px(5.0))
                                        .text_size(px(11.0))
                                        .text_color(rgb(TEXT))
                                        .child("Crossovers"),
                                )
                                .child(self.inspector_row(band, [P::Low, P::High], cx))
                                .child(self.inspector_row(band, [P::LowSlope, P::HighSlope], cx)),
                        ),
                )
                .children(self.detector_open.then(|| {
                    self.inspector_row(
                        band,
                        DETECTOR
                            .iter()
                            .copied()
                            .filter(|field| external || !matches!(field, P::KeyFilter)),
                        cx,
                    )
                    .border_t_1()
                    .border_color(rgb(DIVIDER))
                }));
        Some(panel.into_any_element())
    }

    fn mode_selectors(&self, values: &Values, cx: &mut Context<Self>) -> Div {
        let pair = |axis: ModeAxis,
                    group: &'static str,
                    labels: [(&'static str, &'static str); 2],
                    cx: &mut Context<Self>| {
            let axis_index = axis as usize;
            div()
                .id(("mode-axis", axis_index))
                .role(Role::Group)
                .aria_label(group)
                .flex()
                .gap(px(2.0))
                .children(
                    labels
                        .into_iter()
                        .enumerate()
                        .map(|(option, (text, label))| {
                            let second = option == 1;
                            let active = members(self.selection)
                                .all(|band| axis.active(values, band) == second);
                            let focus = &self.mode_focus[axis_index][option];
                            div()
                                .id(("mode-option", option))
                                .role(Role::Button)
                                .aria_label(label)
                                .aria_toggled(if active {
                                    Toggled::True
                                } else {
                                    Toggled::False
                                })
                                .track_focus(focus)
                                .tab_stop(true)
                                .h(px(24.0))
                                .min_w(px(24.0))
                                .px(px(6.0))
                                .flex()
                                .items_center()
                                .justify_center()
                                .rounded(px(3.0))
                                .border_1()
                                .border_color(rgb(if active { 0x803a34 } else { HAIR }))
                                .bg(rgb(if active { 0x381f1d } else { PAPER }))
                                .text_color(rgb(if active { ACCENT } else { MUTED }))
                                .text_size(px(11.0))
                                .hover(|style| style.bg(rgb(0x2e2e2e)))
                                .focus_visible(|style| style.border_color(rgb(TEXT)))
                                .cursor_pointer()
                                .child(text)
                                .on_click(cx.listener(move |this, _, window, cx| {
                                    window.focus(&this.mode_focus[axis_index][option], cx);
                                    this.select_mode(axis, second, cx);
                                }))
                                .on_key_down(cx.listener(
                                    move |this, event: &toybox::gpui::KeyDownEvent, window, cx| {
                                        let second = match event.keystroke.key.as_str() {
                                            "enter" | "space" => second,
                                            "left" => false,
                                            "right" => true,
                                            "down" if matches!(axis, ModeAxis::Direction) => false,
                                            "up" if matches!(axis, ModeAxis::Direction) => true,
                                            _ => return,
                                        };
                                        window.focus(
                                            &this.mode_focus[axis_index][usize::from(second)],
                                            cx,
                                        );
                                        this.select_mode(axis, second, cx);
                                        cx.stop_propagation();
                                    },
                                ))
                        }),
                )
        };
        div()
            .flex()
            .flex_none()
            .items_center()
            .gap(px(6.0))
            .pl(px(4.0))
            .pr(px(5.0))
            .child(pair(
                ModeAxis::Family,
                "Mode",
                [("Compress", "Compress"), ("Expand", "Expand")],
                cx,
            ))
            .child(pair(
                ModeAxis::Direction,
                "Direction",
                [("↓", "Downward"), ("↑", "Upward")],
                cx,
            ))
    }

    fn select_mode(&mut self, axis: ModeAxis, second: bool, cx: &mut Context<Self>) {
        self.entry = None;
        self.edit_many(axis.edits(&self.params.values(), self.selection, second));
        cx.notify();
    }

    fn inspector_row(
        &self,
        band: usize,
        fields: impl IntoIterator<Item = P>,
        cx: &mut Context<Self>,
    ) -> Div {
        div().h(px(SMALL_ROW)).flex_none().flex().children(
            fields
                .into_iter()
                .map(|field| self.value(field.id(band), ValueStyle::Compact, cx)),
        )
    }

    fn detector_toggle(&self, cx: &mut Context<Self>) -> AnyElement {
        let focus = self.detector_focus.clone();
        let editor = cx.entity().downgrade();
        Icon::Detector
            .button("Detector", self.detector_open)
            .aria_expanded(self.detector_open)
            .track_focus(&focus)
            .tab_stop(true)
            .w_auto()
            .gap(px(4.0))
            .px(px(6.0))
            .text_size(px(11.0))
            .text_color(rgb(if self.detector_open { ACCENT } else { MUTED }))
            .child("Detector")
            .on_a11y_action(AccessibleAction::Click, move |_, _, cx| {
                editor.update(cx, Self::toggle_detector).ok();
            })
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, _, window, cx| {
                    window.focus(&focus, cx);
                    this.toggle_detector(cx);
                    cx.stop_propagation();
                }),
            )
            .on_key_down(
                cx.listener(|this, event: &toybox::gpui::KeyDownEvent, _, cx| {
                    if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                        this.toggle_detector(cx);
                        cx.stop_propagation();
                    }
                }),
            )
            .into_any_element()
    }

    fn toggle_detector(&mut self, cx: &mut Context<Self>) {
        self.entry = None;
        self.detector_open = !self.detector_open;
        cx.notify();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn panel_tracks_band_horizontally_at_a_fixed_bottom() {
        let chart = Bounds::new(point(px(219.0), px(64.0)), size(px(1_280.0), px(566.0)));
        for detector in [false, true] {
            let left = Placement::at_bottom(chart, chart.left(), detector);
            let center = Placement::at_bottom(chart, chart.center().x, detector);
            let right = Placement::at_bottom(chart, chart.right(), detector);
            let [left, center, right] = [left, center, right].map(|panel| panel.bounds(chart));

            assert_eq!(left.left(), chart.left() + px(MARGIN));
            assert_eq!(center.center().x, chart.center().x);
            assert_eq!(right.right(), chart.right() - px(MARGIN));
            assert!(left.left() < center.left() && center.left() < right.left());
            for bounds in [left, center, right] {
                assert_eq!(bounds.top(), center.top());
                assert_eq!(bounds.bottom(), chart.bottom() - px(BOTTOM));
            }
        }
    }

    #[test]
    fn expanded_panel_keeps_its_bottom_and_uses_the_available_width() {
        let chart = Bounds::new(point(px(219.0), px(64.0)), size(px(650.0), px(490.0)));
        let collapsed = Placement::at_bottom(chart, chart.center().x, false);
        let expanded = Placement::at_bottom(chart, chart.center().x, true);
        let bounds = expanded.bounds(chart);

        assert_eq!(bounds.left(), chart.left() + px(12.0));
        assert_eq!(bounds.right(), chart.right() - px(12.0));
        assert_eq!(bounds.bottom(), collapsed.bounds(chart).bottom());
        assert_eq!(
            collapsed.bounds(chart).top() - bounds.top(),
            px(SMALL_ROW + ROW_GAP)
        );
        assert!(expanded.contains(chart, bounds.center()));
        assert!(!expanded.contains(
            chart,
            point(bounds.center().x, chart.bottom() - px(plot::FOOT + 14.0))
        ));
    }
}
