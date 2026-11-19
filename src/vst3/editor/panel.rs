use super::{knob, plot, Command, Editor, ACCENT, BG, HAIR, MUTED};
use crate::parameters::{detector::Source, BandParam as P, Values};
use std::sync::atomic::Ordering::Relaxed;
use toybox::gpui::{
    div, point, px, rgb, size, AnyElement, Bounds, Context, InteractiveElement, IntoElement,
    MouseButton, ParentElement, Pixels, Point, Styled,
};

const WIDTH: f32 = 688.0;
const MARGIN: f32 = 38.0;
const BOTTOM: f32 = plot::FOOT + 8.0;
const PADDING: f32 = 6.0;
const HEADER: f32 = 24.0;
const MAIN_ROW: f32 = 76.0;
const SMALL_ROW: f32 = 36.0;
const ROW_GAP: f32 = 4.0;
const DYNAMICS: [&[P]; 2] = [
    &[
        P::Mode,
        P::Threshold,
        P::Range,
        P::Attack,
        P::Release,
        P::Makeup,
    ],
    &[
        P::Ratio,
        P::Knee,
        P::Lookahead,
        P::Low,
        P::LowSlope,
        P::High,
        P::HighSlope,
    ],
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
        let small_rows = (DYNAMICS.len() - 1 + usize::from(detector)) as f32;
        let height = px((PADDING + 1.0) * 2.0
            + HEADER
            + MAIN_ROW
            + ROW_GAP
            + small_rows * (SMALL_ROW + ROW_GAP));
        let left = (band_x - chart.left() - width / 2.0)
            .clamp(margin, (chart.size.width - margin - width).max(margin));
        Self {
            left,
            width,
            height,
        }
    }

    fn bounds(self, chart: Bounds<Pixels>) -> Bounds<Pixels> {
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
        let enabled = values[P::Enabled.id(band)] > 0.5;
        let external = Source::read(values, band) == Source::External;
        let inactive = external && self.meters.sidechain.load(Relaxed) == 0;
        let panel = div()
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
                    .gap(px(6.0))
                    .child(div().size(px(6.0)).rounded(px(3.0)).bg(rgb(ACCENT)))
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
                    .child(self.band_button(
                        P::Enabled.id(band),
                        if enabled { "On" } else { "Off" },
                        ACCENT,
                        cx,
                    ))
                    .child(self.switch_button(P::Solo.id(band), "Solo", ACCENT, Some(0), cx))
                    .child(self.switch_button(P::Mute.id(band), "Mute", ACCENT, Some(1), cx))
                    .child(self.switch_button(P::Audition.id(band), "Key", ACCENT, Some(2), cx))
                    .child(self.detector_toggle(cx))
                    .child(self.chart_button(Command::Remove, cx))
                    .child(self.chart_button(Command::Deselect, cx)),
            )
            .children(
                DYNAMICS
                    .into_iter()
                    .chain(self.detector_open.then_some(DETECTOR))
                    .enumerate()
                    .map(|(index, row)| {
                        let (height, style) = if index == 0 {
                            (MAIN_ROW, knob::Style::Main)
                        } else {
                            (SMALL_ROW, knob::Style::Small)
                        };
                        div()
                            .h(px(height))
                            .flex_none()
                            .flex()
                            .gap(px(4.0))
                            .children(
                                row.iter()
                                    .filter(|field| {
                                        external || field.id(band) != P::KeyFilter.id(band)
                                    })
                                    .map(|field| self.value(field.id(band), Some(style), cx)),
                            )
                    }),
            );
        Some(panel.into_any_element())
    }

    fn detector_toggle(&self, cx: &mut Context<Self>) -> AnyElement {
        let focus = self.detector_focus.clone();
        let label = if self.detector_open {
            "Detector ▾"
        } else {
            "Detector ▸"
        };
        div()
            .id("detector-controls")
            .track_focus(&focus)
            .tab_stop(true)
            .focus(|style| style.border_color(rgb(ACCENT)))
            .border_1()
            .border_color(rgb(HAIR))
            .rounded(px(4.0))
            .hover(|style| style.bg(rgb(0x393939)))
            .cursor_pointer()
            .px(px(6.0))
            .py(px(3.0))
            .text_size(px(11.0))
            .child(label)
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, _, window, cx| {
                    window.focus(&focus, cx);
                    this.entry = None;
                    this.detector_open = !this.detector_open;
                    cx.stop_propagation();
                    cx.notify();
                }),
            )
            .on_key_down(
                cx.listener(|this, event: &toybox::gpui::KeyDownEvent, _, cx| {
                    if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                        this.entry = None;
                        this.detector_open = !this.detector_open;
                        cx.stop_propagation();
                        cx.notify();
                    }
                }),
            )
            .into_any_element()
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
        assert!(bounds.top() < collapsed.bounds(chart).top());
        assert!(expanded.contains(chart, bounds.center()));
        assert!(!expanded.contains(
            chart,
            point(bounds.center().x, chart.bottom() - px(plot::FOOT))
        ));
    }
}
