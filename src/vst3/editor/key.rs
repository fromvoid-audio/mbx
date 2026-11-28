use super::plot;
use crate::parameters::{
    detector::{self, Source},
    layout::Layout,
    spec, BandParam as P, Values, BANDS,
};
use toybox::gpui::{fill, point, px, rgb, size, Bounds, Pixels, Point, Window};

const GAP: f64 = 0.002;
pub const LANE_Y: f32 = 10.0;
const HIT_RADIUS: f32 = 8.0;
const IDS: [P; 2] = [P::KeyLow, P::KeyHigh];

#[derive(Clone, Copy)]
pub(super) struct Range {
    band: usize,
    edges: [f64; 2],
    stored: [f64; 2],
    detach: Option<(usize, f64, f64)>,
}

impl Range {
    pub fn read(values: &Values, band: usize) -> Self {
        let source = Source::read(values, band);
        let linked = source.follows_band(values, band);
        let stored = IDS.map(|field| values[field.id(band)]);
        Self {
            band,
            edges: if linked {
                Layout::new(values).edges[band]
            } else {
                detector::custom_range(values, band)
            },
            stored,
            detach: if source == Source::Band {
                let id = P::Key.id(band);
                Some((id, values[id], spec(id).normalized(1.0)))
            } else if linked {
                let id = P::KeyFilter.id(band);
                Some((id, values[id], 0.0))
            } else {
                None
            },
        }
    }

    pub fn edge(self, side: usize) -> Edge {
        Edge { range: self, side }
    }

    pub fn hit(
        self,
        bounds: Bounds<Pixels>,
        view: plot::Viewport,
        cursor: Point<Pixels>,
    ) -> Option<usize> {
        if (cursor.y - lane_y(bounds)).abs() > px(HIT_RADIUS) {
            return None;
        }
        (0..2)
            .filter(|&side| view.contains(self.edges[side]))
            .filter_map(|side| {
                let x = plot::x(bounds, view, self.edges[side]);
                let distance = (cursor.x - x).abs();
                (distance <= px(HIT_RADIUS)).then_some((
                    distance,
                    side != usize::from(cursor.x > x),
                    side,
                ))
            })
            .min()
            .map(|(_, _, side)| side)
    }

    pub fn draw(
        self,
        window: &mut Window,
        bounds: Bounds<Pixels>,
        view: plot::Viewport,
        active: Option<usize>,
    ) {
        let [left, right] = self.edges.map(|edge| plot::x(bounds, view, edge));
        let left_visible = left.max(bounds.left() + px(plot::PAD));
        let right_visible = right.min(bounds.right() - px(plot::PAD));
        let y = lane_y(bounds);
        let quad = |window: &mut Window, color, x, y, width, height| {
            window.paint_quad(fill(
                Bounds::new(point(x, y), size(width, height)),
                rgb(color),
            ));
        };
        if left_visible <= right_visible {
            quad(
                window,
                if self.detach.is_some() {
                    0x646464
                } else {
                    0xb4b4b4
                },
                left_visible,
                y,
                right_visible - left_visible,
                px(1.0),
            );
        }
        for (side, x) in [left, right].into_iter().enumerate() {
            if !view.contains(self.edges[side]) {
                continue;
            }
            let active = active == Some(side);
            let top = bounds.top() + px(plot::GRID_INSET);
            let bottom = bounds.top() + px(plot::ruler(bounds) - 14.0);
            let dash = if active { 0x818181 } else { 0x585858 };
            for i in 0..(f32::from(bottom - top) / 10.0) as usize {
                quad(window, dash, x, top + px(i as f32 * 10.0), px(1.0), px(4.0));
            }
            let grip = if active { 0xff453a } else { 0xe8e8e8 };
            quad(window, grip, x - px(1.5), y - px(5.0), px(3.0), px(11.0));
            let inside = x + if side == 0 { px(1.0) } else { px(-5.0) };
            quad(window, grip, inside, y - px(5.0), px(4.0), px(2.0));
        }
    }
}

fn lane_y(bounds: Bounds<Pixels>) -> Pixels {
    bounds.bottom() - px(LANE_Y)
}

#[derive(Clone, Copy)]
pub(super) struct Edge {
    range: Range,
    side: usize,
}

impl Edge {
    pub fn id(self) -> usize {
        IDS[self.side].id(self.range.band)
    }

    pub fn value(self) -> f64 {
        self.range.edges[self.side]
    }

    pub fn grouped(self) -> bool {
        self.range.detach.is_some() || self.range.stored[0] > self.range.stored[1]
    }

    pub fn ids(self) -> impl Iterator<Item = usize> {
        IDS.into_iter()
            .enumerate()
            .filter(move |(side, _)| self.grouped() || self.side == *side)
            .map(move |(_, field)| field.id(self.range.band))
            .chain(self.range.detach.map(|(id, _, _)| id))
    }

    pub fn edits(self, wanted: f64) -> impl Iterator<Item = (usize, f64)> {
        let [low, high] = self.range.edges;
        let value = if self.side == 0 {
            wanted.clamp(0.0, (high - GAP).max(low))
        } else {
            wanted.clamp((low + GAP).min(high), 1.0)
        };
        let changed = (value - self.value()).abs() > 1e-12;
        let mut edges = self.range.stored;
        if changed {
            edges = self.range.edges;
            edges[self.side] = value;
        }
        IDS.into_iter()
            .zip(edges)
            .enumerate()
            .filter(move |(side, _)| self.grouped() || self.side == *side)
            .map(move |(_, (field, edge))| (field.id(self.range.band), edge))
            .chain(
                self.range
                    .detach
                    .map(|(id, before, after)| (id, if changed { after } else { before })),
            )
    }
}

pub fn edge_for_id(values: &Values, id: usize) -> Option<Edge> {
    (0..BANDS).find_map(|band| {
        IDS.iter()
            .position(|field| field.id(band) == id)
            .map(|side| Range::read(values, band).edge(side))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parameters;

    fn apply(values: &mut Values, edits: impl Iterator<Item = (usize, f64)>) {
        for (id, value) in edits {
            values[id] = value;
        }
    }

    #[test]
    fn editing_a_linked_edge_seeds_its_partner_and_preserves_the_external_source() {
        let band = 1;
        let mut values = parameters::defaults();
        let source = P::Key.id(band);
        let filter = P::KeyFilter.id(band);
        for external in [false, true] {
            values[source] = f64::from(external);
            values[filter] = 1.0;
            let original = values;
            let [low, high] = Layout::new(&values).edges[band];
            let edge = Range::read(&values, band).edge(0);
            assert_eq!(edge.value(), low);
            apply(&mut values, edge.edits(low));
            assert_eq!(values, original);
            apply(&mut values, edge.edits(high + 0.1));
            assert_eq!(values[source], if external { 1.0 } else { 0.5 });
            assert_eq!(detector::custom_range(&values, band), [high - GAP, high]);
            if external {
                assert_eq!(values[filter], 0.0);
            }
            apply(&mut values, edge.edits(low));
            assert_eq!(values, original);
        }
    }

    #[test]
    fn custom_edges_are_independent_and_old_reversed_cutoffs_can_be_edited() {
        let mut values = parameters::defaults();
        let band = 1;
        values[P::Key.id(band)] = 0.5;
        let [low, high] = IDS.map(|field| field.id(band));
        values[low] = 0.2;
        values[high] = 0.6;
        let edge = Range::read(&values, band).edge(1);
        assert_eq!(edge.ids().collect::<Vec<_>>(), vec![high]);
        assert_eq!(edge.edits(0.4).collect::<Vec<_>>(), vec![(high, 0.4)]);
        values.swap(low, high);
        let edge = Range::read(&values, band).edge(1);
        apply(&mut values, edge.edits(0.4));
        assert_eq!([values[low], values[high]], [0.2, 0.4]);
    }

    #[test]
    fn key_grips_track_zoom_and_have_their_own_hit_band() {
        let values = parameters::defaults();
        let range = Range::read(&values, 1);
        let view = plot::Viewport::default().move_from(range.edges[0], 0.5, 0.0);
        let bounds = Bounds::new(point(px(19.0), px(40.0)), size(px(920.0), px(530.0)));
        let x = plot::x(bounds, view, range.edges[0]);
        assert_eq!(range.hit(bounds, view, point(x, lane_y(bounds))), Some(0));
        let ruler_bottom = bounds.top() + px(plot::ruler(bounds) + 5.0);
        assert_eq!(range.hit(bounds, view, point(x, ruler_bottom)), None);
    }
}
