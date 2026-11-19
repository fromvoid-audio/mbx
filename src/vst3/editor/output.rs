use super::{Editor, ACCENT, BG, HAIR, MUTED, PAPER, TEXT};
use toybox::gpui::{
    canvas, div, fill, point, px, relative, rgb, size, AnyElement, Bounds, Context,
    InteractiveElement, IntoElement, KeyDownEvent, MouseButton, ParentElement, Pixels, Styled,
};

const FLOOR: f32 = -80.0;
const SCALE_FLOOR: f32 = -60.0;
const TICKS: [i32; 7] = [0, -6, -12, -24, -36, -48, -60];
const RAIL: f32 = 75.0;
const TOP: f32 = 9.0;
const BOTTOM: f32 = 10.0;
const BAR_X: f32 = 9.0;
const BAR_W: f32 = 10.0;
const BAR_GAP: f32 = 5.0;

#[derive(Clone, Copy)]
pub(super) struct Levels {
    db: [f32; 2],
    hold: [f32; 2],
    channels: usize,
}

impl Default for Levels {
    fn default() -> Self {
        Self {
            db: [FLOOR; 2],
            hold: [FLOOR; 2],
            channels: 2,
        }
    }
}

impl Levels {
    pub fn update(&mut self, channels: u8, peak: [f32; 2]) {
        let channels = usize::from(channels.clamp(1, 2));
        if self.channels != channels {
            *self = Self {
                channels,
                ..Self::default()
            };
        }
        for (index, sample) in peak.into_iter().take(channels).enumerate() {
            let db = (20.0 * sample.max(0.0001).log10()).min(24.0);
            self.db[index] = db.max(self.db[index] - 1.2);
            self.hold[index] = self.hold[index].max(db);
        }
    }

    fn clear(&mut self) {
        self.hold = [FLOOR; 2];
    }

    fn peak(self) -> f32 {
        self.hold[0].max(self.hold[1])
    }
}

fn height(db: f32) -> f32 {
    ((db - SCALE_FLOOR) / -SCALE_FLOOR).clamp(0.0, 1.0)
}

impl Editor {
    pub(super) fn output_meter(&self, cx: &mut Context<Self>) -> AnyElement {
        let levels = self.output;
        let graph = canvas(
            |_, _, _| (),
            move |bounds, (), window, _| {
                let top = bounds.top() + px(TOP);
                let bottom = bounds.bottom() - px(BOTTOM);
                let travel = (bottom - top).max(px(1.0));
                let mut block = |x: Pixels, y: Pixels, width: f32, height: Pixels, color| {
                    window.paint_quad(fill(
                        Bounds::new(point(x, y), size(px(width), height)),
                        rgb(color),
                    ));
                };
                for tick in TICKS {
                    let y = bottom - travel * height(tick as f32);
                    block(bounds.left() + px(39.0), y, 5.0, px(1.0), HAIR);
                }
                for i in 0..levels.channels {
                    let x = bounds.left() + px(BAR_X + i as f32 * (BAR_W + BAR_GAP));
                    block(x, top, BAR_W, travel, 0x303030);
                    let fill_height = travel * height(levels.db[i]);
                    if fill_height > px(0.0) {
                        block(x, bottom - fill_height, BAR_W, fill_height, 0xbebebe);
                        let hot = (fill_height - travel * height(-6.0)).max(px(0.0));
                        if hot > px(0.0) {
                            block(x, top + (travel - fill_height), BAR_W, hot, ACCENT);
                        }
                    }
                    if levels.hold[i] > SCALE_FLOOR {
                        let y = (bottom - travel * height(levels.hold[i])).max(top);
                        block(
                            x,
                            y,
                            BAR_W,
                            px(2.0),
                            if levels.hold[i] >= 0.0 { ACCENT } else { TEXT },
                        );
                    }
                    block(
                        x,
                        top - px(5.0),
                        BAR_W,
                        px(2.0),
                        if levels.hold[i] >= 0.0 { ACCENT } else { HAIR },
                    );
                }
            },
        )
        .absolute()
        .inset_0()
        .size_full();
        div()
            .w(px(RAIL))
            .flex_none()
            .flex()
            .flex_col()
            .min_h_0()
            .rounded(px(5.0))
            .border_1()
            .border_color(rgb(HAIR))
            .bg(rgb(BG))
            .child(
                div()
                    .pt(px(5.0))
                    .text_center()
                    .text_size(px(10.0))
                    .text_color(rgb(MUTED))
                    .child("OUT dBFS"),
            )
            .child(
                div().flex_1().min_h_0().relative().child(graph).child(
                    div()
                        .absolute()
                        .top(px(TOP))
                        .bottom(px(BOTTOM))
                        .right(px(5.0))
                        .w(px(23.0))
                        .text_size(px(9.0))
                        .line_height(px(11.0))
                        .text_color(rgb(MUTED))
                        .children(TICKS.map(|db| {
                            div()
                                .absolute()
                                .top(relative(1.0 - height(db as f32)))
                                .mt(px(-5.0))
                                .right_0()
                                .child(db.to_string())
                        })),
                ),
            )
            .child(
                div()
                    .flex()
                    .pl(px(BAR_X))
                    .gap(px(BAR_GAP))
                    .text_size(px(9.0))
                    .text_color(rgb(MUTED))
                    .children((0..levels.channels).map(|index| {
                        div()
                            .w(px(BAR_W))
                            .text_center()
                            .child(if levels.channels == 1 {
                                "M"
                            } else if index == 0 {
                                "L"
                            } else {
                                "R"
                            })
                    })),
            )
            .child(self.output_peak(cx))
            .into_any_element()
    }

    fn output_peak(&self, cx: &mut Context<Self>) -> AnyElement {
        let peak = self.output.peak();
        let text = if peak > FLOOR {
            format!("{peak:+.1}")
        } else {
            "—".to_owned()
        };
        let focus = self.peak_focus.clone();
        div()
            .id("peak-hold")
            .track_focus(&focus)
            .tab_stop(true)
            .rounded_b(px(4.0))
            .border_t_1()
            .border_color(rgb(HAIR))
            .bg(rgb(PAPER))
            .px(px(4.0))
            .py(px(5.0))
            .text_center()
            .text_size(px(10.0))
            .text_color(rgb(if peak >= 0.0 { ACCENT } else { MUTED }))
            .focus(|style| style.border_color(rgb(ACCENT)).text_color(rgb(TEXT)))
            .hover(|style| style.bg(rgb(0x303030)))
            .cursor_pointer()
            .child(format!("Peak {text}"))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |editor, _, window, cx| {
                    window.focus(&focus, cx);
                    editor.output.clear();
                    cx.notify();
                }),
            )
            .on_key_down(cx.listener(|editor, event: &KeyDownEvent, _, cx| {
                if matches!(event.keystroke.key.as_str(), "space" | "enter") {
                    editor.output.clear();
                    cx.stop_propagation();
                    cx.notify();
                }
            }))
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn peak_hold_resets_and_the_unused_lane_clears_in_mono() {
        let mut levels = Levels::default();
        levels.update(2, [0.5, 1.1]);
        assert!(levels.hold[0] < -6.0);
        assert!(levels.hold[1] > 0.0);
        levels.update(2, [0.0; 2]);
        assert!(levels.db[1] < 0.0);
        assert!(levels.peak() > 0.0);
        levels.clear();
        assert_eq!(levels.peak(), FLOOR);
        levels.update(1, [0.3, 1.2]);
        assert_eq!(levels.channels, 1);
        assert!(levels.peak() < 0.0);
        assert_eq!(levels.hold[1], FLOOR);
    }

    #[test]
    fn dbfs_ticks_mark_the_meter_floor_and_ceiling() {
        assert_eq!(height(-80.0), 0.0);
        assert_eq!(height(-60.0), 0.0);
        assert_eq!(height(-30.0), 0.5);
        assert_eq!(height(0.0), 1.0);
        assert_eq!(height(12.0), 1.0);
    }
}
