use super::{
    key,
    response::{BandShape, Response},
    selection::{self, BandSet},
};
use crate::{
    parameters::{self, layout::Layout, BandParam as P, Values, BANDS},
    vst3::meters::{POINTS, TRACES},
};
use toybox::gpui::{self, fill, point, px, rgb, size, Bounds, PathBuilder, Pixels, Point, Window};

pub const FOOT: f32 = 90.0;
pub const LANE: f32 = 11.0;
pub const PAD: f32 = 34.0;
pub const GRID_INSET: f32 = 17.0;
pub const FREQUENCIES: [f64; 10] = [
    20.0, 50.0, 100.0, 200.0, 500.0, 1000.0, 2000.0, 5000.0, 10_000.0, 20_000.0,
];
pub const LABELS: [&str; 10] = [
    "20", "50", "100", "200", "500", "1k", "2k", "5k", "10k", "20k",
];
pub const ACCENT: u32 = 0xff453a;

pub fn normalized(hz: f64) -> f64 {
    (hz / 20.0).ln() / 1000.0_f64.ln()
}

pub use crate::parameters::layout::edge_ids;

pub fn edges(values: &Values, band: usize) -> [f64; 2] {
    Layout::new(values).edges[band]
}

#[derive(Clone, Copy)]
pub struct Viewport {
    left: f64,
    width: f64,
    gain_db: f32,
}

impl Default for Viewport {
    fn default() -> Self {
        Self {
            left: 0.0,
            width: 1.0,
            gain_db: 24.0,
        }
    }
}

impl Viewport {
    pub fn position(self, frequency: f64) -> f64 {
        (frequency - self.left) / self.width
    }
    pub fn frequency(self, position: f64) -> f64 {
        self.left + self.width * position
    }
    pub fn contains(self, frequency: f64) -> bool {
        (0.0..=1.0).contains(&self.position(frequency))
    }

    pub fn gain(self, gain_db: f32) -> Self {
        Self { gain_db, ..self }
    }

    pub fn db_pixels(self, bounds: Bounds<Pixels>) -> f32 {
        ((ruler(bounds) - 2.0 * GRID_INSET) * 0.5).max(1.0) / self.gain_db
    }

    pub fn gain_ticks(self) -> impl Iterator<Item = (f32, String)> {
        (0..=4).map(move |index| {
            let position = index as f32 / 4.0;
            let db = self.gain_db * (1.0 - 2.0 * position);
            let label = if index == 2 {
                "0".to_owned()
            } else {
                format!("{db:+.0}")
            };
            (position, label)
        })
    }

    pub fn move_from(self, anchor: f64, zoom: f64, pan: f64) -> Self {
        let width = (self.width * zoom).clamp(0.16, 1.0);
        let offset = self.position(anchor).clamp(0.0, 1.0);
        let left = (anchor + (pan - offset) * width).clamp(0.0, 1.0 - width);
        Self {
            left,
            width,
            ..self
        }
    }
}

pub fn ruler(bounds: Bounds<Pixels>) -> f32 {
    f32::from(bounds.size.height) - FOOT
}

pub fn at_x(bounds: Bounds<Pixels>, view: Viewport, x: Pixels) -> f64 {
    let width = f32::from(bounds.size.width) - PAD * 2.0;
    let position = f32::from(x - bounds.left() - px(PAD)) / width.max(1.0);
    view.frequency(f64::from(position).clamp(0.0, 1.0))
}

pub(super) fn x(bounds: Bounds<Pixels>, view: Viewport, normalized: f64) -> Pixels {
    bounds.left() + px(PAD) + (bounds.size.width - px(PAD * 2.0)) * view.position(normalized) as f32
}

fn rect(window: &mut Window, x: Pixels, y: Pixels, w: Pixels, h: Pixels, color: u32) {
    window.paint_quad(fill(Bounds::new(point(x, y), size(w, h)), rgb(color)));
}

pub fn range_direction(values: &Values, band: usize) -> f32 {
    let id = P::Mode.id(band);
    let mode = parameters::spec(id).plain(values[id]) as u8;
    if mode.is_multiple_of(2) {
        -1.0
    } else {
        1.0
    }
}

struct Region {
    shape: BandShape,
    makeup: f32,
    limit: f32,
}

impl Region {
    fn new(values: &Values, band: usize, edges: [f64; 2]) -> Self {
        let id = P::Makeup.id(band);
        let makeup = parameters::spec(id).plain(values[id]) as f32;
        let limit = makeup + selection::range_db(values, band) as f32;
        Self {
            shape: BandShape::new(values, band, edges),
            makeup,
            limit,
        }
    }

    fn center(&self, bounds: Bounds<Pixels>, view: Viewport) -> Point<Pixels> {
        point(
            x(bounds, view, self.shape.center()),
            db_y(bounds, view, self.makeup),
        )
    }
}

pub fn band_anchor(
    bounds: Bounds<Pixels>,
    view: Viewport,
    values: &Values,
    band: usize,
) -> (Point<Pixels>, Pixels) {
    let region = Region::new(values, band, edges(values, band));
    (
        region.center(bounds, view),
        db_y(bounds, view, region.limit),
    )
}

fn db_y(bounds: Bounds<Pixels>, view: Viewport, db: f32) -> Pixels {
    bounds.top()
        + px(1.0 + ruler(bounds) * 0.5
            - db.clamp(-view.gain_db, view.gain_db) * view.db_pixels(bounds))
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Hit {
    Center(usize),
    Edge(usize, usize),
    Range(usize),
    Body(usize),
}

impl Hit {
    pub fn band(self) -> usize {
        match self {
            Self::Center(band) | Self::Edge(band, _) | Self::Range(band) | Self::Body(band) => band,
        }
    }
}

pub fn hit(
    bounds: Bounds<Pixels>,
    view: Viewport,
    values: &Values,
    selected: usize,
    cursor: Point<Pixels>,
) -> Option<Hit> {
    let layout = Layout::new(values);
    let ordered = || {
        std::iter::once(selected)
            .filter(|band| layout.order().contains(band))
            .chain(
                layout
                    .order()
                    .iter()
                    .copied()
                    .filter(|&band| band != selected),
            )
    };
    let near = px(7.0);
    for band in ordered() {
        let shape = Region::new(values, band, layout.edges[band]);
        let center = shape.center(bounds, view);
        if view.contains(shape.shape.center())
            && (cursor.x - center.x).abs() <= near
            && (cursor.y - center.y).abs() <= near
        {
            return Some(Hit::Center(band));
        }
    }
    let frequency = at_x(bounds, view, cursor.x);
    for band in ordered() {
        let shape = Region::new(values, band, layout.edges[band]);
        let [left, right] = shape.shape.edges.map(|edge| x(bounds, view, edge));
        let weight = shape.shape.contour_weight(frequency);
        let makeup = db_y(bounds, view, shape.makeup * weight);
        let limit = db_y(bounds, view, shape.limit * weight);
        let inside_y = (makeup.min(limit) - near..=makeup.max(limit) + near).contains(&cursor.y);
        if let Some((side, edge)) = shape
            .shape
            .edges
            .into_iter()
            .enumerate()
            .filter(|&(_, edge)| view.contains(edge))
            .min_by_key(|&(_, edge)| (cursor.x - x(bounds, view, edge)).abs())
        {
            let grip_y = db_y(
                bounds,
                view,
                shape.makeup * shape.shape.contour_weight(edge),
            );
            if (cursor.x - x(bounds, view, edge)).abs() <= near
                && (inside_y || (cursor.y - grip_y).abs() <= near)
            {
                return Some(Hit::Edge(band, side));
            }
        }
        // The curves can extend past the crossover. Each frequency still
        // belongs to at most one band's selectable area.
        if (left..=right).contains(&cursor.x) && inside_y {
            if (cursor.y - limit).abs() <= near {
                return Some(Hit::Range(band));
            }
            return Some(Hit::Body(band));
        }
    }
    None
}

pub fn range_drag_weight(
    bounds: Bounds<Pixels>,
    view: Viewport,
    values: &Values,
    band: usize,
    cursor_x: Pixels,
) -> f64 {
    let shape = BandShape::new(values, band, edges(values, band));
    f64::from(shape.contour_weight(at_x(bounds, view, cursor_x)).max(0.2))
}

fn stroke(window: &mut Window, points: &[Point<Pixels>], color: gpui::Hsla, width: f32) {
    let mut path = PathBuilder::stroke(px(width));
    path.move_to(points[0]);
    for &point in &points[1..] {
        path.line_to(point);
    }
    if let Ok(path) = path.build() {
        window.paint_path(path, color);
    }
}

fn region(window: &mut Window, bounds: Bounds<Pixels>, band: BandDisplay, view: Viewport) {
    let BandDisplay {
        region,
        selected,
        enabled,
        activity,
    } = band;
    let color = match (enabled, selected) {
        (true, true) => ACCENT,
        (true, false) => 0xa4a4a4,
        (false, true) => 0x878787,
        (false, false) => 0x5a5a5a,
    };
    const SAMPLES: usize = 160;
    let mut baseline = [point(px(0.0), px(0.0)); SAMPLES];
    let mut limit = baseline;
    let mut movement = baseline;
    for index in 0..SAMPLES {
        let frequency = view.frequency(index as f64 / (SAMPLES - 1) as f64);
        let weight = region.shape.contour_weight(frequency);
        let x = x(bounds, view, frequency);
        baseline[index] = point(x, db_y(bounds, view, region.makeup * weight));
        limit[index] = point(x, db_y(bounds, view, region.limit * weight));
        movement[index] = point(x, db_y(bounds, view, (region.makeup + activity) * weight));
    }
    let mut area = PathBuilder::fill();
    area.move_to(baseline[0]);
    for &point in baseline[1..].iter().chain(limit.iter().rev()) {
        area.line_to(point);
    }
    area.close();
    if let Ok(path) = area.build() {
        let mut tint: gpui::Hsla = rgb(color).into();
        tint.a = if selected { 0.20 } else { 0.065 };
        window.paint_path(path, tint);
    }
    stroke(
        window,
        &limit,
        rgb(color).into(),
        if selected { 1.5 } else { 1.0 },
    );
    if enabled && activity.abs() > 0.08 {
        let mut live: gpui::Hsla = rgb(color).into();
        live.a = if selected { 0.95 } else { 0.65 };
        stroke(window, &movement, live, 1.7);
    }
}

struct BandDisplay {
    region: Region,
    selected: bool,
    enabled: bool,
    activity: f32,
}

fn handles(window: &mut Window, bounds: Bounds<Pixels>, view: Viewport, band: &BandDisplay) {
    let BandDisplay {
        region,
        selected,
        enabled,
        ..
    } = band;
    let color = if *enabled && *selected {
        ACCENT
    } else if *enabled {
        0xb4b4b4
    } else {
        0x5e5e5e
    };
    let circle = |window: &mut Window, center: Point<Pixels>, radius: f32, color| {
        let extent = px(radius * 2.0);
        let origin = point(center.x - px(radius), center.y - px(radius));
        window.paint_quad(
            fill(Bounds::new(origin, size(extent, extent)), rgb(color)).corner_radii(px(radius)),
        );
    };
    if view.contains(region.shape.center()) {
        let center = region.center(bounds, view);
        circle(window, center, if *selected { 7.0 } else { 5.0 }, 0x111111);
        circle(window, center, if *selected { 5.0 } else { 3.5 }, color);
        if *selected && (region.limit - region.makeup).abs() > 0.05 {
            let y = db_y(bounds, view, region.limit);
            rect(
                window,
                center.x - px(4.0),
                y - px(1.0),
                px(8.0),
                px(3.0),
                color,
            );
        }
    }
    if *selected {
        for edge in region.shape.edges {
            if view.contains(edge) && edge > 0.0 && edge < 1.0 {
                let center = point(
                    x(bounds, view, edge),
                    db_y(
                        bounds,
                        view,
                        region.makeup * region.shape.contour_weight(edge),
                    ),
                );
                rect(
                    window,
                    center.x - px(1.5),
                    center.y - px(5.0),
                    px(3.0),
                    px(10.0),
                    color,
                );
            }
        }
    }
}

fn curve(
    window: &mut Window,
    bounds: Bounds<Pixels>,
    view: Viewport,
    trace: &[f32; POINTS],
    color: u32,
    width: f32,
) {
    if trace.iter().all(|&db| db <= -90.0) {
        return;
    }
    let mut path = PathBuilder::stroke(px(width));
    for index in 0..POINTS {
        let frequency = view.frequency(index as f64 / (POINTS - 1) as f64);
        let point_index = frequency * (POINTS - 1) as f64;
        let i = (point_index as usize).min(POINTS - 1);
        let db =
            trace[i] + (trace[(i + 1).min(POINTS - 1)] - trace[i]) * point_index.fract() as f32;
        let y = bounds.top() + px(16.0 + (-db.clamp(-90.0, 0.0) / 90.0) * (ruler(bounds) - 30.0));
        let p = point(x(bounds, view, frequency), y);
        if index == 0 {
            path.move_to(p);
        } else {
            path.line_to(p);
        }
    }
    if let Ok(path) = path.build() {
        window.paint_path(path, rgb(color));
    }
}

pub struct Display<'a> {
    pub view: Viewport,
    pub values: &'a Values,
    pub selected: Option<usize>,
    pub selection: BandSet,
    pub draft: Option<f64>,
    pub marquee: Option<[Point<Pixels>; 2]>,
    pub detector: Option<key::Range>,
    pub detector_edge: Option<usize>,
    pub spectra: &'a [[f32; POINTS]; TRACES],
    pub shown: [bool; TRACES],
    pub activity: [f32; BANDS],
}

pub fn draw(window: &mut Window, bounds: Bounds<Pixels>, display: Display<'_>) {
    let Display {
        view,
        values,
        selected,
        selection,
        draft,
        marquee,
        detector,
        detector_edge,
        spectra,
        shown,
        activity,
    } = display;
    let width = bounds.size.width;
    let left = bounds.left();
    let top = bounds.top();
    let grid = 0x383838;
    for index in 0..=4 {
        let y = top + px(GRID_INSET + (ruler(bounds) - 2.0 * GRID_INSET) * index as f32 / 4.0);
        rect(
            window,
            left + px(PAD),
            y,
            width - px(PAD * 2.0),
            px(1.0),
            grid,
        );
    }
    for frequency in FREQUENCIES
        .into_iter()
        .map(normalized)
        .filter(|freq| view.contains(*freq))
    {
        let x = x(bounds, view, frequency);
        rect(
            window,
            x,
            top + px(16.0),
            px(1.0),
            px(ruler(bounds) - 30.0),
            grid,
        );
    }
    if shown[0] {
        curve(window, bounds, view, &spectra[0], 0xc8c8c8, 1.0);
    }
    if shown[1] {
        curve(window, bounds, view, &spectra[1], ACCENT, 1.7);
    }
    if shown[2] {
        curve(window, bounds, view, &spectra[2], 0xd0928d, 1.0);
    }
    let layout = Layout::new(values);
    let bands = layout
        .order()
        .iter()
        .copied()
        .filter(|&band| Some(band) != selected)
        .chain(selected)
        .map(|band| BandDisplay {
            region: Region::new(values, band, layout.edges[band]),
            selected: selection & selection::one(band) != 0,
            enabled: values[P::Enabled.id(band)] > 0.5,
            activity: activity[band],
        });
    for band in bands.clone() {
        region(window, bounds, band, view);
    }
    let response = Response::new(values, activity);
    let mut gain = PathBuilder::stroke(px(1.3));
    for index in 0..POINTS {
        let frequency = view.frequency(index as f64 / (POINTS - 1) as f64);
        let pos = point(
            x(bounds, view, frequency),
            db_y(bounds, view, response.db(frequency)),
        );
        if index == 0 {
            gain.move_to(pos);
        } else {
            gain.line_to(pos);
        }
    }
    if let Ok(path) = gain.build() {
        window.paint_path(path, rgb(0xf0d4d2));
    }
    for band in bands {
        handles(window, bounds, view, &band);
    }
    if let Some(frequency) = draft {
        let x = x(bounds, view, frequency);
        for segment in 0..((ruler(bounds) - 40.0) / 9.0) as usize {
            rect(
                window,
                x,
                top + px(29.0 + segment as f32 * 9.0),
                px(1.0),
                px(4.0),
                0xa54f49,
            );
        }
    }
    if let Some([start, end]) = marquee {
        let rectangle = marquee_bounds(start, end);
        let mut tint: gpui::Hsla = rgb(ACCENT).into();
        tint.a = 0.15;
        window.paint_quad(fill(rectangle, tint));
    }
    rect(
        window,
        left + px(PAD),
        top + px(ruler(bounds)),
        width - px(PAD * 2.0),
        px(1.0),
        grid,
    );
    for (lane, &band) in layout.order().iter().enumerate() {
        let color = if selection & selection::one(band) != 0 {
            ACCENT
        } else {
            0xa4a4a4
        };
        let [low, high] = layout.edges[band].map(|value| x(bounds, view, value));
        if high < bounds.left() + px(PAD) || low > bounds.right() - px(PAD) {
            continue;
        }
        let low = low.max(bounds.left() + px(PAD));
        let high = high.min(bounds.right() - px(PAD));
        let enabled = values[P::Enabled.id(band)] > 0.5;
        let row = top + px(ruler(bounds) + 10.0 + LANE * lane as f32);
        let line = if enabled { color } else { 0x515151 };
        let edge = if Some(band) == selected { color } else { line };
        rect(
            window,
            left + px(PAD),
            row + px(2.0),
            width - px(PAD * 2.0),
            px(1.0),
            0x2b2b2b,
        );
        rect(
            window,
            low,
            row + px(1.0),
            (high - low).max(px(1.0)),
            px(3.0),
            line,
        );
        rect(window, low - px(1.0), row - px(1.0), px(3.0), px(7.0), edge);
        rect(
            window,
            high - px(1.0),
            row - px(1.0),
            px(3.0),
            px(7.0),
            edge,
        );
    }
    for edge in selected
        .into_iter()
        .flat_map(|band| edges(values, band))
        .filter(|&edge| view.contains(edge))
        .map(|edge| x(bounds, view, edge))
    {
        let mut color: gpui::Hsla = rgb(ACCENT).into();
        color.a = 0.25;
        window.paint_quad(fill(
            Bounds::new(
                point(edge, top + px(16.0)),
                size(px(1.0), px(ruler(bounds) - 30.0)),
            ),
            color,
        ));
    }
    if let Some(range) = detector {
        range.draw(window, bounds, view, detector_edge);
    }
}

fn marquee_bounds(start: Point<Pixels>, end: Point<Pixels>) -> Bounds<Pixels> {
    Bounds::new(
        point(start.x.min(end.x), start.y.min(end.y)),
        size((start.x - end.x).abs(), (start.y - end.y).abs()),
    )
}

pub fn enclosed(
    bounds: Bounds<Pixels>,
    view: Viewport,
    values: &Values,
    start: Point<Pixels>,
    end: Point<Pixels>,
) -> BandSet {
    let area = marquee_bounds(start, end);
    let layout = Layout::new(values);
    layout
        .order()
        .iter()
        .copied()
        .filter(|&band| {
            let region = Region::new(values, band, layout.edges[band]);
            area.contains(&region.center(bounds, view))
        })
        .fold(0, |set, band| set | selection::one(band))
}

pub fn range_text(values: &Values, band: usize) -> String {
    let [low, high] = edges(values, band).map(|value| 20.0 * 1000_f64.powf(value));
    let format_hz = |hz: f64| format!("{hz:.0}");
    let format_khz = |hz: f64| {
        let text = format!("{:.2}", hz / 1000.0);
        text.trim_end_matches('0').trim_end_matches('.').to_owned()
    };
    if high < 1000.0 {
        format!("{}–{} Hz", format_hz(low), format_hz(high))
    } else if low >= 1000.0 {
        format!("{}–{} kHz", format_khz(low), format_khz(high))
    } else {
        format!("{} Hz–{} kHz", format_hz(low), format_khz(high))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bounds() -> Bounds<Pixels> {
        Bounds::new(point(px(0.0), px(0.0)), size(px(820.0), px(326.0)))
    }

    #[test]
    fn chart_picks_enabled_band_center_edge_and_range() {
        let mut values = parameters::empty();
        let bounds = bounds();
        let band = 2;
        let view = Viewport::default();
        let coords = Region::new(&values, band, edges(&values, band));
        let center = coords.center(bounds, view);
        assert_eq!(hit(bounds, view, &values, band, center), None);
        values[P::Enabled.id(band)] = 1.0;
        assert_eq!(
            hit(bounds, view, &values, band, center),
            Some(Hit::Center(band))
        );
        assert_eq!(
            hit(
                bounds,
                view,
                &values,
                band,
                point(x(bounds, view, coords.shape.edges[0]), center.y)
            ),
            Some(Hit::Edge(band, 0))
        );
        assert_eq!(
            hit(
                bounds,
                view,
                &values,
                band,
                point(center.x, db_y(bounds, view, coords.limit))
            ),
            Some(Hit::Range(band))
        );
    }

    #[test]
    fn range_hits_follow_the_curve_on_either_side_of_zero() {
        let bounds = bounds();
        let view = Viewport::default();
        let mut values = parameters::defaults();
        let band = 1;
        let edges = edges(&values, band);
        let frequency = edges[0] + 0.014;
        for db in [-20.0, 20.0] {
            for (id, value) in selection::range_edits(&values, band, db) {
                values[id] = value;
            }
            let region = Region::new(&values, band, edges);
            let weight = region.shape.contour_weight(frequency);
            assert!(weight < 0.8);
            let x = x(bounds, view, frequency);
            let limit = point(x, db_y(bounds, view, region.limit * weight));
            assert_eq!(
                hit(bounds, view, &values, band, limit),
                Some(Hit::Range(band))
            );
            let old_box = point(x, db_y(bounds, view, region.limit));
            assert_eq!(hit(bounds, view, &values, band, old_box), None);
        }
    }

    #[test]
    fn expanded_spectrum_keeps_the_ruler_inset_and_gain_handles_pickable() {
        let mut expanded = bounds();
        expanded.size.height += px(200.0);
        let values = parameters::defaults();
        let view = Viewport::default();
        let region = Region::new(&values, 1, edges(&values, 1));
        assert_eq!(expanded.size.height - px(ruler(expanded)), px(FOOT));
        assert_eq!(
            region.center(expanded, view).y - region.center(bounds(), view).y,
            px(100.0)
        );
        assert_eq!(
            hit(expanded, view, &values, 1, region.center(expanded, view)),
            Some(Hit::Center(1))
        );
    }

    #[test]
    fn zoom_anchors_and_pan_respects_spectrum_limits() {
        let view = Viewport::default().move_from(0.43, 0.5, 0.0);
        assert!((view.position(0.43) - 0.43).abs() < 1e-12);
        assert_eq!(view.move_from(0.43, 1.0, -8.0).frequency(0.0), 0.0);
        assert_eq!(view.move_from(0.43, 1.0, 8.0).frequency(1.0), 1.0);
    }
}
