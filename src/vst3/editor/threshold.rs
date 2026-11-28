use super::{entry::Entry, plot, ACCENT, MUTED, PAPER, TEXT};
use crate::parameters::Spec;
use toybox::gpui::{
    div, point, px, rgb, size, AnyElement, Bounds, Div, InteractiveElement, IntoElement,
    MouseButton, Orientation, ParentElement, Pixels, Point, Role, Stateful,
    StatefulInteractiveElement, Styled,
};

const WIDTH: f32 = 198.0;
const HEIGHT: f32 = 22.0;
const BORDER: u32 = 0x82514b;
const DOT_DIAMETER: f32 = 12.0;

#[derive(Clone, Copy)]
pub(super) struct DetectorReading {
    level_db: f32,
    reacting: bool,
}

impl DetectorReading {
    pub fn new(level_db: f32, gain_db: f32, enabled: bool) -> Self {
        Self {
            level_db,
            reacting: enabled && gain_db.abs() >= 0.05,
        }
    }

    fn readout(self) -> Div {
        let level = if self.level_db <= -90.0 {
            "≤−90 dB".to_owned()
        } else {
            format!("{:.1} dB", self.level_db)
        };
        div()
            .flex_none()
            .flex()
            .items_center()
            .gap(px(3.0))
            .child(div().text_color(rgb(MUTED)).child("DET"))
            .child(
                div()
                    .text_color(rgb(if self.reacting { 0xffb5ac } else { TEXT }))
                    .child(level),
            )
    }
}

#[derive(Clone, Copy)]
pub(super) struct Marker {
    chart: Bounds<Pixels>,
    span: [Pixels; 2],
    y: Pixels,
    detector_x: Pixels,
    pub grip: Bounds<Pixels>,
    pub drag_pixels: f32,
}

impl Marker {
    pub fn new(
        chart: Bounds<Pixels>,
        view: plot::Viewport,
        edges: [f64; 2],
        spec: Spec,
        value: f64,
        inspector: Bounds<Pixels>,
    ) -> Option<Self> {
        let left = chart.left() + px(plot::PAD);
        let right = chart.right() - px(plot::PAD);
        let [start, end] = edges.map(|edge| plot::x(chart, view, edge));
        let span = [start.max(left), end.min(right)];
        if span[0] >= span[1] {
            return None;
        }
        let width = px(WIDTH).min(right - left);
        let center = span[0] + (span[1] - span[0]) * 0.25;
        let detector_x = span[0] + (span[1] - span[0]) * 0.78;
        let mut x = (center - width / 2.0).clamp(left, right - width);
        let dot_gap = px(DOT_DIAMETER / 2.0 + 4.0);
        if x + width > detector_x - dot_gap && x < detector_x + dot_gap {
            if detector_x - dot_gap - width >= left {
                x = detector_x - dot_gap - width;
            } else if detector_x + dot_gap + width <= right {
                x = detector_x + dot_gap;
            }
        }
        let y = plot::spectrum_y(chart, spec.plain(value) as f32);
        let mut grip = Bounds::new(point(x, y - px(HEIGHT / 2.0)), size(width, px(HEIGHT)));
        // The readout stays reachable when the inspector covers the actual level.
        if grip.intersects(&inspector) {
            grip.origin.y = inspector.top() - px(HEIGHT + 4.0);
        }
        let travel = plot::spectrum_y(chart, spec.plain(0.0) as f32)
            - plot::spectrum_y(chart, spec.plain(1.0) as f32);
        Some(Self {
            chart,
            span,
            y,
            detector_x,
            grip,
            drag_pixels: f32::from(travel).max(1.0),
        })
    }

    pub fn hits_line(self, cursor: Point<Pixels>) -> bool {
        (self.span[0]..=self.span[1]).contains(&cursor.x) && (cursor.y - self.y).abs() <= px(5.0)
    }

    pub fn line(self, focused: bool, detector: DetectorReading) -> Div {
        div()
            .absolute()
            .left(self.span[0] - self.chart.left())
            .top(self.y - self.chart.top() - px(1.0))
            .w(self.span[1] - self.span[0])
            .h(px(2.0))
            .border_t_2()
            .border_dashed()
            .border_color(rgb(if detector.reacting {
                ACCENT
            } else if focused {
                0xffb5ac
            } else {
                0xc58b82
            }))
    }

    fn detector_position(self, detector: DetectorReading) -> Point<Pixels> {
        point(
            self.detector_x,
            plot::spectrum_y(self.chart, detector.level_db),
        )
    }

    pub fn detector_dot(self, detector: DetectorReading) -> Div {
        let position = self.detector_position(detector);
        let radius = px(DOT_DIAMETER / 2.0);
        div()
            .absolute()
            .left(position.x - self.chart.left() - radius)
            .top(position.y - self.chart.top() - radius)
            .size(px(DOT_DIAMETER))
            .p(px(1.0))
            .rounded_full()
            .bg(rgb(0x111111))
            .child(
                div()
                    .size_full()
                    .rounded_full()
                    .border_1()
                    .border_color(rgb(0xffded9))
                    .bg(rgb(if detector.reacting { ACCENT } else { PAPER })),
            )
    }

    pub fn control(
        self,
        control: Stateful<Div>,
        spec: Spec,
        value: f64,
        entry: Option<&Entry>,
        display: String,
        detector: DetectorReading,
    ) -> AnyElement {
        let plain = spec.plain(value);
        control
            .role(Role::Slider)
            .aria_label("Band threshold in spectrum")
            .aria_orientation(Orientation::Vertical)
            .aria_numeric_value(plain)
            .aria_value(spec.display(value))
            .aria_min_numeric_value(spec.plain(0.0))
            .aria_max_numeric_value(spec.plain(1.0))
            .absolute()
            .left(self.grip.left() - self.chart.left())
            .top(self.grip.top() - self.chart.top())
            .w(self.grip.size.width)
            .h(self.grip.size.height)
            .py_0()
            .px(px(5.0))
            .bg(rgb(PAPER))
            .border_color(rgb(if entry.is_some() { ACCENT } else { BORDER }))
            .flex()
            .items_center()
            .justify_center()
            .gap(px(3.0))
            .text_size(px(10.0))
            .line_height(px(14.0))
            .whitespace_nowrap()
            .cursor_ns_resize()
            .on_mouse_down(MouseButton::Right, |_, _, cx| cx.stop_propagation())
            .on_scroll_wheel(|_, _, cx| cx.stop_propagation())
            .child(
                div()
                    .w(px(8.0))
                    .h(px(3.0))
                    .flex_none()
                    .rounded(px(1.5))
                    .bg(rgb(ACCENT)),
            )
            .child(div().flex_none().text_color(rgb(MUTED)).child("THR"))
            .child(
                div()
                    .min_w_0()
                    .overflow_hidden()
                    .text_color(rgb(if entry.is_some_and(|entry| entry.invalid) {
                        ACCENT
                    } else {
                        TEXT
                    }))
                    .child(display),
            )
            .children(
                (self.y > self.grip.bottom()).then(|| div().text_color(rgb(MUTED)).child("↓")),
            )
            .child(
                div()
                    .w(px(1.0))
                    .h(px(12.0))
                    .mx(px(1.0))
                    .flex_none()
                    .bg(rgb(BORDER)),
            )
            .child(detector.readout())
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parameters::{self, BandParam};

    #[test]
    fn detector_highlight_follows_metered_gain_in_either_direction() {
        assert!(DetectorReading::new(-22.0, -0.4, true).reacting);
        assert!(DetectorReading::new(-26.0, 0.4, true).reacting);
        assert!(!DetectorReading::new(-22.0, 0.0, true).reacting);
        assert!(!DetectorReading::new(-22.0, -0.01, true).reacting);
        assert!(!DetectorReading::new(-22.0, -0.4, false).reacting);
    }

    #[test]
    fn threshold_tracks_spectrum_level_and_clips_to_visible_band() {
        let chart = Bounds::new(point(px(18.0), px(40.0)), size(px(800.0), px(550.0)));
        let inspector = Bounds::new(point(px(50.0), px(375.0)), size(px(720.0), px(175.0)));
        let view = plot::Viewport::default().move_from(0.5, 0.5, 0.0);
        let spec = parameters::spec(BandParam::Threshold.id(0));
        let marker = Marker::new(
            chart,
            view,
            [0.3, 0.8],
            spec,
            spec.normalized(-24.0),
            inspector,
        )
        .unwrap();
        assert_eq!(marker.y, plot::spectrum_y(chart, -24.0));
        assert_eq!(marker.grip.center().y, marker.y);
        assert_eq!(marker.span[1], chart.right() - px(plot::PAD));
        assert!(marker.hits_line(point(marker.span[1] - px(6.0), marker.y + px(2.0))));
        assert!(!marker.hits_line(point(marker.span[0] - px(1.0), marker.y)));
        assert!(!marker.hits_line(point(marker.grip.center().x, marker.y + px(7.0))));
        assert!(Marker::new(chart, view, [0.0, 0.1], spec, 1.0, inspector).is_none());
        let dot = |level| marker.detector_position(DetectorReading::new(level, 0.0, true));
        assert_eq!(dot(-24.0).y, marker.y);
        assert!(dot(-12.0).y < marker.y && dot(-36.0).y > marker.y);
        assert_eq!(dot(-12.0).x, dot(-36.0).x);
        assert!((marker.span[0]..=marker.span[1]).contains(&dot(-24.0).x));
        let narrow = Marker::new(
            chart,
            view,
            [0.26, 0.29],
            spec,
            spec.normalized(-24.0),
            inspector,
        )
        .unwrap();
        assert!(!narrow
            .grip
            .contains(&narrow.detector_position(DetectorReading::new(-24.0, 0.0, true))));

        let deep = Marker::new(chart, view, [0.3, 0.8], spec, 0.0, inspector).unwrap();
        assert_eq!(deep.y, plot::spectrum_y(chart, -72.0));
        assert_eq!(deep.grip.bottom(), inspector.top() - px(4.0));
        assert_eq!(
            deep.drag_pixels,
            f32::from(deep.y - plot::spectrum_y(chart, 0.0))
        );
    }
}
