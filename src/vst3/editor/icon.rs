use super::{ACCENT, BG, HAIR, MUTED, TEXT};
use toybox::gpui::{
    canvas, div, point, px, rgb, AnyElement, AppContext, Context, Div, InteractiveElement,
    IntoElement, ParentElement, PathBuilder, Render, Role, Stateful, StatefulInteractiveElement,
    Styled, Window,
};

#[derive(Clone, Copy)]
pub(super) enum Icon {
    Power,
    Solo,
    Mute,
    Audition,
    Detector,
    Remove,
    Close,
}

impl Icon {
    pub fn button(self, label: &'static str, active: bool) -> Stateful<Div> {
        div()
            .id(label)
            .role(Role::Button)
            .aria_label(label)
            .tooltip(move |_, cx| cx.new(|_| Tooltip(label)).into())
            .size(px(24.0))
            .flex_none()
            .flex()
            .items_center()
            .justify_center()
            .rounded(px(4.0))
            .border_1()
            .border_color(rgb(if active { 0x803a34 } else { BG }))
            .bg(rgb(if active { 0x381f1d } else { BG }))
            .hover(|style| style.bg(rgb(0x2e2e2e)))
            .focus_visible(|style| style.border_color(rgb(TEXT)))
            .cursor_pointer()
            .child(self.draw(if active { ACCENT } else { MUTED }))
    }

    fn draw(self, color: u32) -> AnyElement {
        let lines: &[&[(f32, f32)]] = match self {
            Self::Power => &[
                &[(8.0, 1.5), (8.0, 7.0)],
                &[
                    (4.8, 4.0),
                    (2.6, 6.0),
                    (2.0, 9.2),
                    (3.3, 12.2),
                    (6.3, 14.0),
                    (9.7, 14.0),
                    (12.7, 12.2),
                    (14.0, 9.2),
                    (13.4, 6.0),
                    (11.2, 4.0),
                ],
            ],
            Self::Solo => &[
                &[
                    (2.0, 10.0),
                    (2.0, 7.5),
                    (3.0, 4.5),
                    (5.2, 2.8),
                    (8.0, 2.3),
                    (10.8, 2.8),
                    (13.0, 4.5),
                    (14.0, 7.5),
                    (14.0, 10.0),
                ],
                &[(2.0, 9.0), (5.0, 9.0), (5.0, 13.5), (2.0, 13.5), (2.0, 9.0)],
                &[
                    (14.0, 9.0),
                    (11.0, 9.0),
                    (11.0, 13.5),
                    (14.0, 13.5),
                    (14.0, 9.0),
                ],
            ],
            Self::Mute => &[
                &[
                    (1.7, 6.0),
                    (4.0, 6.0),
                    (7.6, 3.0),
                    (7.6, 13.0),
                    (4.0, 10.0),
                    (1.7, 10.0),
                    (1.7, 6.0),
                ],
                &[(10.5, 5.8), (14.6, 10.2)],
                &[(14.6, 5.8), (10.5, 10.2)],
            ],
            Self::Audition => &[
                &[
                    (6.4, 8.0),
                    (4.5, 8.6),
                    (2.5, 7.7),
                    (1.6, 5.5),
                    (2.5, 3.2),
                    (4.5, 2.3),
                    (6.7, 3.2),
                    (7.6, 5.4),
                    (7.1, 7.3),
                    (14.0, 13.8),
                ],
                &[(12.0, 12.0), (14.0, 10.0)],
            ],
            Self::Detector => &[&[
                (2.0, 12.5),
                (4.1, 12.5),
                (6.4, 4.0),
                (9.6, 4.0),
                (11.9, 12.5),
                (14.0, 12.5),
            ]],
            Self::Remove => &[
                &[(2.8, 4.4), (13.2, 4.4)],
                &[(6.1, 4.4), (6.1, 2.4), (9.9, 2.4), (9.9, 4.4)],
                &[(4.2, 6.1), (4.9, 14.0), (11.1, 14.0), (11.8, 6.1)],
                &[(8.0, 7.2), (8.0, 11.8)],
            ],
            Self::Close => &[&[(4.0, 4.0), (12.0, 12.0)], &[(12.0, 4.0), (4.0, 12.0)]],
        };
        canvas(
            |_, _, _| (),
            move |bounds, (), window, _| {
                let position = |(x, y)| point(bounds.left() + px(x), bounds.top() + px(y));
                let mut path = PathBuilder::stroke(px(1.35));
                for line in lines {
                    if let Some((&first, rest)) = line.split_first() {
                        path.move_to(position(first));
                        for &next in rest {
                            path.line_to(position(next));
                        }
                    }
                }
                if let Ok(path) = path.build() {
                    window.paint_path(path, rgb(color));
                }
            },
        )
        .size(px(16.0))
        .flex_none()
        .into_any_element()
    }
}

struct Tooltip(&'static str);

impl Render for Tooltip {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .px(px(7.0))
            .py(px(3.0))
            .rounded(px(4.0))
            .border_1()
            .border_color(rgb(HAIR))
            .bg(rgb(BG))
            .text_color(rgb(TEXT))
            .text_size(px(11.0))
            .child(self.0)
    }
}
