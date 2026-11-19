//! Rotary rendering for continuous controls in the band panel.

use super::{ACCENT, TEXT};
use std::f32::consts::{FRAC_PI_4, PI};
use toybox::gpui::{
    canvas, fill, point, px, rgb, size, AnyElement, Bounds, IntoElement, PathBuilder, Pixels,
    Point, Styled, Window,
};

#[derive(Clone, Copy)]
pub(super) enum Style {
    Main,
    Small,
}

pub(super) fn dial(style: Style, value: f32, neutral: f32, detector: Option<f32>) -> AnyElement {
    let diameter = match style {
        Style::Main => 40.0,
        Style::Small => 20.0,
    };
    canvas(
        |_, _, _| (),
        move |bounds, (), window, _| {
            let center = bounds.center();
            let radius = diameter / 2.0;
            let face = radius
                - if matches!(style, Style::Main) {
                    6.0
                } else {
                    5.0
                };
            window.paint_quad(
                fill(
                    Bounds::new(
                        point(center.x - px(face), center.y - px(face)),
                        size(px(face * 2.0), px(face * 2.0)),
                    ),
                    rgb(0x303030),
                )
                .corner_radii(px(face))
                .border_widths(px(1.0))
                .border_color(rgb(0x4e4e4e)),
            );
            let ring = radius - 2.5;
            arc(window, center, ring, 0.0, 1.0, 0x454545);
            arc(
                window,
                center,
                ring,
                neutral.min(value),
                neutral.max(value),
                ACCENT,
            );
            if let Some(level) = detector {
                tick(window, center, level, ring - 2.6, ring + 2.0, TEXT);
            }
            tick(window, center, value, face * 0.37, face - 3.0, TEXT);
        },
    )
    .size(px(diameter))
    .flex_none()
    .into_any_element()
}

fn polar(center: Point<Pixels>, radius: f32, value: f32) -> Point<Pixels> {
    let angle = -3.0 * FRAC_PI_4 + value * (1.5 * PI);
    point(
        center.x + px(angle.sin() * radius),
        center.y - px(angle.cos() * radius),
    )
}

fn arc(window: &mut Window, center: Point<Pixels>, radius: f32, start: f32, end: f32, color: u32) {
    if end - start < 0.002 {
        return;
    }
    let mut path = PathBuilder::stroke(px(2.0));
    path.move_to(polar(center, radius, start));
    for step in 1..=32 {
        let value = start + (end - start) * step as f32 / 32.0;
        path.line_to(polar(center, radius, value));
    }
    if let Ok(path) = path.build() {
        window.paint_path(path, rgb(color));
    }
}

fn tick(
    window: &mut Window,
    center: Point<Pixels>,
    value: f32,
    inner: f32,
    outer: f32,
    color: u32,
) {
    let mut path = PathBuilder::stroke(px(1.8));
    path.move_to(polar(center, inner, value));
    path.line_to(polar(center, outer, value));
    if let Ok(path) = path.build() {
        window.paint_path(path, rgb(color));
    }
}

pub(super) fn signed_position(db: f64) -> f32 {
    ((db.clamp(-36.0, 36.0) + 36.0) / 72.0) as f32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn signed_range_has_a_center_and_opposite_arcs() {
        assert_eq!(signed_position(0.0), 0.5);
        assert_eq!(signed_position(-36.0), 0.0);
        assert_eq!(signed_position(36.0), 1.0);
        assert_eq!(signed_position(-12.0) + signed_position(12.0), 1.0);
    }
}
