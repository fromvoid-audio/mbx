use crate::parameters::{
    self,
    layout::{edge_ids, Boundary, Layout},
    BandParam as P, Values, BANDS,
};

pub type BandSet = u8;

pub fn one(band: usize) -> BandSet {
    1 << band
}

pub fn members(set: BandSet) -> impl Iterator<Item = usize> {
    (0..BANDS).filter(move |&band| set & one(band) != 0)
}

pub fn present(values: &Values) -> BandSet {
    Layout::new(values)
        .order()
        .iter()
        .fold(0, |mask, &band| mask | one(band))
}

pub fn between(values: &Values, anchor: usize, target: usize) -> BandSet {
    let layout = Layout::new(values);
    let mut selecting = false;
    let mut set = 0;
    for &band in layout.order() {
        if band == anchor || band == target {
            if selecting || anchor == target {
                return set | one(band);
            }
            selecting = true;
        }
        if selecting {
            set |= one(band);
        }
    }
    set
}

pub fn boundaries(values: &Values, set: BandSet) -> Vec<Boundary> {
    let layout = Layout::new(values);
    let mut boundaries: Vec<Boundary> = Vec::new();
    for band in members(set) {
        for side in 0..2 {
            let boundary = layout.boundary(values, band, side);
            if boundary.movable() && !boundaries.iter().any(|other| boundary.same_as(*other)) {
                boundaries.push(boundary);
            }
        }
    }
    boundaries
}

pub fn group_delta(boundaries: &[Boundary], delta: f64) -> f64 {
    let [low, high] = boundaries
        .iter()
        .fold([-1.0_f64, 1.0_f64], |[low, high], boundary| {
            [
                low.max(boundary.limits[0] - boundary.value),
                high.min(boundary.limits[1] - boundary.value),
            ]
        });
    delta.clamp(low, high)
}

pub fn split(values: &Values, frequency: f64) -> Option<(usize, Vec<(usize, f64)>)> {
    let layout = Layout::new(values);
    let band = (0..BANDS).find(|&band| !parameters::present(values, band))?;
    let base = parameters::defaults();
    let mut changes = Vec::new();
    let (parent, [low, high]) = if layout.order().is_empty() {
        (None, [0.0, 1.0])
    } else {
        let parent = layout.at(frequency)?;
        let [_, upper] = layout.edges[parent];
        let ids = edge_ids(values, parent);
        let parent_high_slope = if ids[1] == P::High.id(parent) {
            P::HighSlope
        } else {
            P::LowSlope
        };
        changes.push((ids[1], frequency));
        changes.push((parent_high_slope.id(parent), base[P::HighSlope.id(band)]));
        (Some((parent, parent_high_slope)), [frequency, upper])
    };
    changes.extend(P::ALL.into_iter().map(|field| {
        let id = field.id(band);
        let value = match field {
            P::Low => low,
            P::High => high,
            P::Present => 1.0,
            P::LowSlope => base[id],
            P::HighSlope => parent.map_or(base[id], |(parent, high_slope)| {
                values[high_slope.id(parent)]
            }),
            _ => parent.map_or(base[id], |(parent, _)| values[field.id(parent)]),
        };
        (id, value)
    }));
    Some((band, changes))
}

pub fn merge(values: &Values, mut removed: BandSet) -> Vec<(usize, f64)> {
    let layout = Layout::new(values);
    let Some(&first) = layout.order().first() else {
        return Vec::new();
    };
    if layout.order().iter().all(|&band| removed & one(band) != 0) {
        removed &= !one(first);
    }
    let mut changes = Vec::new();
    let mut leading = None;
    let mut pending_right = None;
    let mut left = None;
    for &band in layout.order() {
        let [low, high] = layout.edges[band];
        if removed & one(band) != 0 {
            if left.is_some() {
                pending_right = Some(high);
            } else {
                leading.get_or_insert(low);
            }
            changes.extend(
                [P::Enabled, P::Solo, P::Present, P::Mute, P::Audition]
                    .map(|field| (field.id(band), 0.0)),
            );
        } else {
            if let Some(low) = leading.take() {
                changes.push((edge_ids(values, band)[0], low));
            }
            if let (Some(previous), Some(high)) = (left, pending_right.take()) {
                changes.push((edge_ids(values, previous)[1], high));
            }
            left = Some(band);
        }
    }
    if let (Some(previous), Some(high)) = (left, pending_right) {
        changes.push((edge_ids(values, previous)[1], high));
    }
    changes
}

pub fn range_band(id: usize) -> Option<usize> {
    (0..BANDS).find(|&band| id == P::Range.id(band))
}

pub fn range_db(values: &Values, band: usize) -> f64 {
    let id = P::Range.id(band);
    parameters::spec(id).plain(values[id]) * f64::from(super::plot::range_direction(values, band))
}

pub fn range_edits(values: &Values, band: usize, db: f64) -> [(usize, f64); 2] {
    let range = P::Range.id(band);
    let mode = P::Mode.id(band);
    let spec = parameters::spec(mode);
    let original = spec.plain(values[mode]) as usize;
    let direction = if db == 0.0 {
        original % 2
    } else {
        usize::from(db > 0.0)
    };
    [
        (mode, spec.normalized((original / 2 * 2 + direction) as f64)),
        (range, parameters::spec(range).normalized(db.abs())),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_and_remove_preserve_a_contiguous_partition_and_clone_processing() {
        let mut values = parameters::defaults();
        let expected = Layout::new(&values).edges[1];
        values[P::Attack.id(1)] = 0.75;
        let cut = (expected[0] + expected[1]) * 0.5;
        let (band, changes) = split(&values, cut).unwrap();
        for (id, value) in changes {
            values[id] = value;
        }
        let layout = Layout::new(&values);
        assert_eq!(layout.order(), &[0, 1, band, 2]);
        assert_eq!(layout.edges[1], [expected[0], cut]);
        assert_eq!(layout.edges[band], [cut, expected[1]]);
        assert_eq!(values[P::Attack.id(band)], 0.75);
        assert_eq!(between(&values, 1, 2), one(1) | one(band) | one(2));
        for (id, value) in merge(&values, one(band)) {
            values[id] = value;
        }
        let layout = Layout::new(&values);
        assert_eq!(layout.edges[1], expected);
        assert_eq!(layout.order(), &[0, 1, 2]);
    }

    #[test]
    fn signed_range_crosses_zero_without_changing_compression_family() {
        let mut values = parameters::defaults();
        let band = 1;
        let mode = P::Mode.id(band);
        let spec = parameters::spec(mode);
        values[mode] = spec.normalized(2.0);
        assert!(range_db(&values, band) < 0.0);
        for (id, value) in range_edits(&values, band, 5.0) {
            values[id] = value;
        }
        assert_eq!(spec.plain(values[mode]), 3.0);
        assert_eq!(range_db(&values, band), 5.0);
    }
}
