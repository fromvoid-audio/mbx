use super::{present, BandParam as P, Values, BANDS};

pub const MIN_WIDTH: f64 = 0.008;
const SHARED: f64 = 1e-8;

#[derive(Clone, Copy)]
pub struct Layout {
    pub edges: [[f64; 2]; BANDS],
    order: [usize; BANDS],
    count: usize,
}

#[derive(Clone, Copy, Debug)]
pub struct Boundary {
    pub ids: [Option<usize>; 2],
    pub value: f64,
    pub limits: [f64; 2],
}

pub fn edge_ids(values: &Values, band: usize) -> [usize; 2] {
    let ids = [P::Low.id(band), P::High.id(band)];
    if values[ids[0]] <= values[ids[1]] {
        ids
    } else {
        [ids[1], ids[0]]
    }
}

impl Layout {
    pub fn new(values: &Values) -> Self {
        let mut edges = std::array::from_fn(|band| edge_ids(values, band).map(|id| values[id]));
        let mut order = [0; BANDS];
        let mut count = 0;
        for band in 0..BANDS {
            if present(values, band) {
                order[count] = band;
                count += 1;
            }
        }
        let centers = edges.map(|[low, high]| (low + high) * 0.5);
        order[..count]
            .sort_unstable_by(|&a, &b| centers[a].total_cmp(&centers[b]).then_with(|| a.cmp(&b)));

        // Legacy ranges and simultaneous automation may overlap. Divide each
        // overlap between the original centers to preserve the ordering.
        for neighbors in order[..count].windows(2) {
            let [left, right] = [neighbors[0], neighbors[1]];
            if edges[left][1] > edges[right][0] {
                let floor = edges[left][0].max(edges[right][0]);
                let ceiling = edges[left][1].min(edges[right][1]);
                let divider = ((centers[left] + centers[right]) * 0.5).clamp(floor, ceiling);
                edges[left][1] = divider;
                edges[right][0] = divider;
            }
        }
        Self {
            edges,
            order,
            count,
        }
    }

    pub fn order(&self) -> &[usize] {
        &self.order[..self.count]
    }

    pub fn at(&self, cut: f64) -> Option<usize> {
        self.order().iter().copied().find(|&band| {
            let [low, high] = self.edges[band];
            high - low > 2.0 * MIN_WIDTH && (low + MIN_WIDTH..=high - MIN_WIDTH).contains(&cut)
        })
    }

    /// Return the stable IDs for this boundary, including an adjoining band
    /// if it shares the edge. Endpoints at the full spectrum limits are fixed.
    pub fn boundary(&self, values: &Values, band: usize, side: usize) -> Boundary {
        let [low, high] = self.edges[band];
        let value = self.edges[band][side];
        let id = edge_ids(values, band)[side];
        let index = self.order().iter().position(|&other| other == band);
        let neighbor = index.and_then(|index| {
            if side == 0 {
                index.checked_sub(1).map(|i| self.order[i])
            } else {
                self.order().get(index + 1).copied()
            }
        });
        let same = neighbor.filter(|&other| (self.edges[other][1 - side] - value).abs() < SHARED);
        let mut limits = if side == 0 {
            [
                neighbor.map_or(0.0, |other| self.edges[other][1]),
                (high - MIN_WIDTH).max(low),
            ]
        } else {
            [
                (low + MIN_WIDTH).min(high),
                neighbor.map_or(1.0, |other| self.edges[other][0]),
            ]
        };
        if let Some(other) = same {
            if side == 0 {
                limits[0] = self.edges[other][0] + MIN_WIDTH;
            } else {
                limits[1] = self.edges[other][1] - MIN_WIDTH;
            }
        } else if (side == 0 && low == 0.0) || (side == 1 && high == 1.0) {
            limits = [value; 2];
        }
        limits[0] = limits[0].min(value);
        limits[1] = limits[1].max(value);
        Boundary {
            ids: [
                Some(id),
                same.map(|other| edge_ids(values, other)[1 - side]),
            ],
            value,
            limits,
        }
    }

    pub fn boundary_by_id(&self, values: &Values, id: usize) -> Option<Boundary> {
        self.order().iter().find_map(|&band| {
            edge_ids(values, band)
                .iter()
                .position(|edge| *edge == id)
                .map(|side| self.boundary(values, band, side))
        })
    }
}

impl Boundary {
    pub fn edits(self, value: f64) -> impl Iterator<Item = (usize, f64)> {
        let value = value.clamp(self.limits[0], self.limits[1]);
        self.ids.into_iter().flatten().map(move |id| (id, value))
    }

    pub fn movable(self) -> bool {
        self.limits[0] < self.limits[1]
    }

    pub fn same_as(self, other: Self) -> bool {
        other
            .ids
            .into_iter()
            .flatten()
            .any(|id| self.ids.contains(&Some(id)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parameters;

    #[test]
    fn default_layout_is_contiguous_and_edges_edit_as_one_boundary() {
        let values = parameters::defaults();
        let layout = Layout::new(&values);
        assert_eq!(layout.order(), &[0, 1, 2]);
        assert_eq!(layout.edges[0][0], 0.0);
        assert_eq!(layout.edges[2][1], 1.0);
        for (left, right) in [(0, 1), (1, 2)] {
            assert_eq!(layout.edges[left][1], layout.edges[right][0]);
            let edge = layout.boundary(&values, left, 1);
            assert_eq!(edge.ids, [Some(P::High.id(left)), Some(P::Low.id(right))]);
            let edits: Vec<_> = edge.edits(0.99).collect();
            assert!(edits[0].1 < layout.edges[right][1]);
            assert_eq!(edits[0].1, edits[1].1);
        }
        assert!(!layout.boundary(&values, 0, 0).movable());
    }

    #[test]
    fn nested_and_crossed_host_ranges_never_overlap() {
        let mut values = parameters::defaults();
        let mut seed = 487_321_u32;
        let mut next = || {
            seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            f64::from(seed & 1023) / 1023.0
        };
        for _ in 0..500 {
            for band in 0..BANDS {
                values[P::Present.id(band)] = 1.0;
                values[P::Low.id(band)] = next();
                values[P::High.id(band)] = next();
            }
            let layout = Layout::new(&values);
            for &band in layout.order() {
                let [lo, hi] = layout.edges[band];
                assert!(lo <= hi);
            }
            for pair in layout.order().windows(2) {
                assert!(layout.edges[pair[0]][1] <= layout.edges[pair[1]][0]);
            }
        }
    }
}
