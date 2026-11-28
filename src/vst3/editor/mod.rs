mod browser;
mod controls;
mod entry;
mod icon;
mod journal;
mod key;
mod knob;
mod output;
mod panel;
mod plot;
mod response;
mod selection;
mod size_controls;
mod threshold;

use selection::{members, one, BandSet};

use super::{
    comparison::{Comparison, Slot},
    controller::{mark_dirty, restart_for_latency, with_group_edit, Handler},
    meters::{Analyzer, Meters, POINTS, TRACES},
    view::{self, Presentation},
    Parameters,
};
use crate::parameters::{
    self,
    detector::Source,
    layout::{Boundary, Layout},
    BandParam as P, Values, BANDS, BYPASS, COUNT, LOOKAHEAD, MIX, OUTPUT,
};
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
    sync::{atomic::Ordering::Relaxed, Arc},
    time::Duration,
};
use toybox::{
    gpui::{
        self, canvas, div, px, relative, rgb, AnyElement, AppContext, Bounds, Context, FocusHandle,
        InteractiveElement, IntoElement, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent,
        ParentElement, Pixels, Point, Render, StatefulInteractiveElement, Styled, Task, Window,
    },
    vst3::prelude::IComponentHandlerTrait,
};

const BG: u32 = 0x171717;
const PAPER: u32 = 0x202020;
const TEXT: u32 = 0xf4f4f4;
const MUTED: u32 = 0xb0b0b0;
const HAIR: u32 = 0x444444;
const RAIL: u32 = 0x101010;
const RAIL_TEXT: u32 = TEXT;
const RAIL_MUTED: u32 = MUTED;
const ACCENT: u32 = plot::ACCENT;
const HEADER_HEIGHT: f32 = 32.0;
const HEADER_PADDING: f32 = 12.0;
const HEADER_GAP: f32 = 8.0;
const TITLE_WIDTH: f32 = 41.0;
const FOOTER_HEIGHT: f32 = 30.0;
const BAR_CONTROL_HEIGHT: f32 = 24.0;
const VERTICAL_DRAG_PIXELS: f32 = 150.0;

type EditIds = RefCell<Vec<usize>>;

#[derive(Clone, Copy)]
enum ValueStyle {
    Footer,
    Knob,
    Compact,
    Threshold(threshold::Marker, threshold::DetectorReading),
}

#[derive(Clone, Copy)]
enum Command {
    Remove,
    Split,
    Undo,
    Redo,
    Deselect,
}

impl Command {
    fn label(self) -> &'static str {
        match self {
            Self::Remove => "Remove",
            Self::Split => "Split band",
            Self::Undo => "Undo",
            Self::Redo => "Redo",
            Self::Deselect => "Close",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Axis {
    Frequency,
    Gain,
}

#[derive(Clone, Copy)]
enum Drag {
    Vertical {
        start: Pixels,
        value: f64,
        drag_pixels: f32,
        boundary: Option<Boundary>,
    },
    Detector {
        start: Point<Pixels>,
        edge: key::Edge,
        horizontal: bool,
        moved: bool,
    },
    Edge {
        start: f64,
        boundary: Boundary,
    },
    Center {
        start: Point<Pixels>,
        group: BandSet,
        axis: Option<Axis>,
    },
    Range {
        start: Pixels,
        group: BandSet,
        db_per_pixel: f64,
    },
    Ruler {
        start: Point<Pixels>,
        initial: plot::Viewport,
        anchor: f64,
    },
    Marquee {
        start: Point<Pixels>,
        end: Point<Pixels>,
    },
}

struct Editor {
    params: Arc<Parameters>,
    handler: Handler,
    meters: Arc<Meters>,
    analyzer: Analyzer,
    spectra: [[f32; POINTS]; TRACES],
    traces: [bool; TRACES],
    freeze: bool,
    output: output::Levels,
    peak_focus: FocusHandle,
    bands_focus: FocusHandle,
    phase_linear_focus: FocusHandle,
    bands_open: bool,
    presentation: Rc<Presentation>,
    size_focus: [FocusHandle; 2],
    panel: Option<panel::Placement>,
    detector_open: bool,
    detector_focus: FocusHandle,
    activity: [f32; BANDS],
    selected: usize,
    selection: BandSet,
    hover: Option<f64>,
    hovered_band: Option<usize>,
    viewport: plot::Viewport,
    gain_scale: usize,
    drag_db_pixels: f32,
    drag_gain_db: f32,
    gain_focus: FocusHandle,
    drag_initial: Values,
    drag_edges: Vec<Boundary>,
    entry: Option<entry::Entry>,
    plot: Rc<Cell<Option<Bounds<Pixels>>>>,
    chart_focus: FocusHandle,
    threshold_focus: FocusHandle,
    threshold_marker: Option<threshold::Marker>,
    focus: [FocusHandle; COUNT],
    band_focus: [FocusHandle; BANDS],
    trace_focus: [FocusHandle; TRACES + 1],
    command_focus: [FocusHandle; 5],
    comparison: Rc<RefCell<Comparison>>,
    browser: browser::Browser,
    preset_focus: [FocusHandle; 7],
    graph_focus: [FocusHandle; 5],
    inspector_switch_focus: [FocusHandle; 4],
    mode_focus: [[FocusHandle; 2]; 2],
    press: Rc<controls::PressSlot>,
    journals: [RefCell<journal::Journal>; 2],
    editing: Rc<EditIds>,
    drag: Option<Drag>,
    _refresh: Task<()>,
}

fn finish(handler: &Handler, editing: &EditIds) {
    let ids = editing.take();
    let handler = handler.borrow().clone();
    if let Some(handler) = handler {
        for id in ids {
            unsafe { handler.endEdit(id as u32) };
        }
    }
}

impl Drop for Editor {
    fn drop(&mut self) {
        controls::cancel_pointer(&self.press, &self.params, &self.handler, &self.editing);
    }
}

impl Editor {
    fn slot(&self) -> Slot {
        self.comparison.borrow().active()
    }

    fn journal(&self) -> &RefCell<journal::Journal> {
        &self.journals[self.slot().index()]
    }

    fn apply_values(&self, values: Values) {
        controls::restore_press(&self.press, &self.params, &self.handler, true);
        self.finish_edit();
        self.edit_many(values.into_iter().enumerate());
    }

    fn switch_to(&mut self, slot: Slot, cx: &mut Context<Self>) {
        controls::restore_press(&self.press, &self.params, &self.handler, true);
        self.finish_edit();
        self.drag = None;
        self.entry = None;
        let current = self.params.values();
        let values = self.comparison.borrow_mut().switch_to(slot, current);
        if let Some(values) = values {
            self.journal().borrow_mut().replaying = true;
            self.apply_values(values);
            self.journal().borrow_mut().replaying = false;
            mark_dirty(&self.handler);
        }
        cx.notify();
    }

    fn finish_edit(&self) {
        finish(&self.handler, &self.editing);
        self.journal().borrow_mut().finish(|id| self.params.get(id));
    }

    fn history_step(&self, redo: bool, cx: &mut Context<Self>) {
        self.finish_edit();
        let edits = self.journal().borrow_mut().step(redo);
        if let Some(edits) = edits {
            self.journal().borrow_mut().replaying = true;
            self.edit_many(edits);
            self.journal().borrow_mut().replaying = false;
        }
        cx.notify();
    }

    fn begin(&self, ids: impl IntoIterator<Item = usize>) {
        self.finish_edit();
        let ids: Vec<_> = ids.into_iter().collect();
        self.journal()
            .borrow_mut()
            .begin(&ids, |id| self.params.get(id));
        self.editing.replace(ids.clone());
        let handler = self.handler.borrow().clone();
        if let Some(handler) = handler {
            for id in ids {
                unsafe { handler.beginEdit(id as u32) };
            }
        }
    }

    fn select(&mut self, band: usize, modifiers: gpui::Modifiers, values: &Values) {
        if modifiers.platform {
            self.selection ^= one(band);
        } else if modifiers.shift && self.selection != 0 {
            self.selection = selection::between(values, self.selected, band);
        } else if self.selection & one(band) == 0 {
            self.selection = one(band);
        }
        self.selected = if self.selection & one(band) == 0 {
            members(self.selection).next().unwrap_or(band)
        } else {
            band
        };
    }

    fn create_band(&mut self, frequency: f64) -> Option<usize> {
        let (band, changes) = selection::split(&self.params.values(), frequency)?;
        self.edit_many(changes);
        self.selection = one(band);
        self.selected = band;
        Some(band)
    }

    fn remove_selected(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let changes = selection::merge(&self.params.values(), self.selection);
        if !changes.is_empty() {
            self.edit_many(changes);
            self.selection = 0;
            self.entry = None;
            window.focus(&self.chart_focus, cx);
            cx.notify();
        }
    }

    fn add_at_cursor(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let layout = Layout::new(&self.params.values());
        let band = layout
            .order()
            .iter()
            .copied()
            .find(|&band| {
                self.selection & one(band) != 0
                    && layout.at(Self::center(&layout, band)) == Some(band)
            })
            .or_else(|| {
                layout.order().iter().copied().max_by(|&a, &b| {
                    let width = |band: usize| layout.edges[band][1] - layout.edges[band][0];
                    width(a).total_cmp(&width(b))
                })
            });
        let frequency = self
            .hover
            .filter(|&x| layout.at(x).is_some())
            .or_else(|| band.map(|band| Self::center(&layout, band)))
            .unwrap_or(0.5);
        if self.create_band(frequency).is_some() {
            window.focus(&self.chart_focus, cx);
            cx.notify();
        }
    }

    fn center(layout: &Layout, band: usize) -> f64 {
        let [low, high] = layout.edges[band];
        (low + high) * 0.5
    }

    fn change(&self, id: usize, value: f64) {
        let value = parameters::spec(id).canonical(value);
        if self.params.get(id) == value {
            return;
        }
        self.params.set(id, value);
        let handler = self.handler.borrow().clone();
        if let Some(handler) = handler {
            unsafe { handler.performEdit(id as u32, value) };
        }
    }

    fn edit(&self, id: usize, value: f64) {
        self.edit_many([(id, value)]);
    }

    fn edit_many(&self, edits: impl IntoIterator<Item = (usize, f64)>) {
        let edits: Vec<_> = edits
            .into_iter()
            .filter(|(id, value)| self.params.get(*id) != parameters::spec(*id).canonical(*value))
            .collect();
        if edits.is_empty() {
            return;
        }
        // Publish both latency-changing controls before any host callbacks.
        // The comparison or preset then triggers a single reactivation.
        let old_config = self.meters.configuration();
        let config = edits.iter().fold(old_config, |config, &(id, value)| {
            config.with_param(id, value)
        });
        if config != old_config {
            self.meters.replace_configuration(config);
        }
        self.finish_edit();
        with_group_edit(&self.handler, edits.len() > 1, || {
            self.begin(edits.iter().map(|&(id, _)| id));
            for (id, value) in edits {
                self.change(id, value);
            }
            self.finish_edit();
        });
        if config != old_config {
            restart_for_latency(&self.handler);
        }
    }

    fn control_ids(&self, id: usize) -> Vec<usize> {
        if self.selection.count_ones() > 1 {
            if let Some(field) = P::ALL
                .into_iter()
                .find(|field| field.id(self.selected) == id)
            {
                return members(self.selection).map(|band| field.id(band)).collect();
            }
        }
        vec![id]
    }

    fn control_set(&self, id: usize, value: f64) {
        let values = self.params.values();
        if let Some(edge) = key::edge_for_id(&values, id) {
            self.edit_many(edge.edits(value));
        } else if let Some(boundary) = Layout::new(&values).boundary_by_id(&values, id) {
            self.edit_many(boundary.edits(value));
        } else {
            self.edit_many(
                self.control_ids(id)
                    .into_iter()
                    .map(|target| (target, value)),
            );
        }
    }

    fn boundary(&self, id: usize) -> Option<Boundary> {
        let values = self.params.values();
        Layout::new(&values).boundary_by_id(&values, id)
    }

    fn reset_control(&self, id: usize) {
        let spec = parameters::spec(id);
        if selection::range_band(id).is_some() {
            let values = self.params.values();
            let db = -spec.plain(spec.default_normalized());
            self.edit_many(
                self.control_ids(id)
                    .into_iter()
                    .filter_map(selection::range_band)
                    .flat_map(|band| selection::range_edits(&values, band, db)),
            );
        } else {
            self.control_set(id, spec.default_normalized());
        }
    }

    fn control_step(&self, id: usize, direction: i32, fine: bool) {
        let spec = parameters::spec(id);
        if spec.choices.is_empty() {
            let step = if fine { 0.002 } else { 0.015 };
            let values = self.params.values();
            if let Some(edge) = key::edge_for_id(&values, id) {
                self.edit_many(edge.edits(edge.value() + f64::from(direction) * step));
                return;
            }
            if selection::range_band(id).is_some() {
                let delta = f64::from(direction) * step * 36.0;
                self.edit_many(
                    self.control_ids(id)
                        .into_iter()
                        .filter_map(selection::range_band)
                        .flat_map(|band| {
                            selection::range_edits(
                                &values,
                                band,
                                selection::range_db(&values, band) + delta,
                            )
                        }),
                );
                return;
            }
            if let Some(boundary) = self.boundary(id) {
                self.edit_many(boundary.edits(boundary.value + f64::from(direction) * step));
                return;
            }
            self.edit_many(self.control_ids(id).into_iter().map(|target| {
                (
                    target,
                    self.params.get(target) + f64::from(direction) * step,
                )
            }));
        } else {
            let current = spec.plain(self.params.get(id)) as i32;
            let next = (current + direction).rem_euclid(spec.choices.len() as i32);
            self.control_set(id, spec.normalized(f64::from(next)));
        }
    }

    fn prepare_number(&mut self, id: usize) {
        let values = self.params.values();
        let plain = if let Some(band) = selection::range_band(id) {
            selection::range_db(&values, band)
        } else {
            let normalized = self.control_value(&values, id);
            parameters::spec(id).plain(normalized)
        };
        self.entry = Some(entry::Entry::new(id, plain));
    }

    fn begin_number(&mut self, id: usize, window: &mut Window, cx: &mut Context<Self>) {
        self.prepare_number(id);
        window.focus(&self.focus[id], cx);
        cx.notify();
    }

    fn control_value(&self, values: &Values, id: usize) -> f64 {
        if let Some(edge) = key::edge_for_id(values, id) {
            edge.value()
        } else {
            Layout::new(values)
                .boundary_by_id(values, id)
                .map_or(values[id], |edge| edge.value)
        }
    }

    fn start_detector(&mut self, start: Point<Pixels>, edge: key::Edge, horizontal: bool) {
        self.begin(edge.ids());
        self.drag = Some(Drag::Detector {
            start,
            edge,
            horizontal,
            moved: false,
        });
    }

    fn detector_range(&self, values: &Values) -> Option<key::Range> {
        (self.detector_open && self.selection != 0 && parameters::present(values, self.selected))
            .then(|| key::Range::read(values, self.selected))
    }

    fn detector_level(&self, band: usize) -> f32 {
        f32::from_bits(self.meters.detector[band].load(Relaxed))
    }

    fn start_range(&mut self, band: usize, start: Pixels, db_per_pixel: f64) {
        let group = if self.selection & one(band) == 0 {
            one(band)
        } else {
            self.selection
        };
        self.drag_initial = self.params.values();
        self.drag_gain_db = self.gain_display(&self.drag_initial);
        self.begin(members(group).flat_map(|band| [P::Mode.id(band), P::Range.id(band)]));
        self.drag = Some(Drag::Range {
            start,
            group,
            db_per_pixel,
        });
    }

    fn entry_key(&mut self, id: usize, key: &gpui::Keystroke, cx: &mut Context<Self>) -> bool {
        let Some(entry) = self.entry.as_mut().filter(|entry| entry.id == id) else {
            return false;
        };
        match key.key.as_str() {
            "escape" => self.entry = None,
            "enter" => {
                let spec = parameters::spec(id);
                if let Some(plain) = entry::parse_plain(spec, &entry.text) {
                    self.entry = None;
                    if selection::range_band(id).is_some() {
                        let values = self.params.values();
                        self.edit_many(
                            self.control_ids(id)
                                .into_iter()
                                .filter_map(selection::range_band)
                                .flat_map(|band| selection::range_edits(&values, band, plain)),
                        );
                    } else {
                        self.control_set(id, spec.normalized(plain));
                    }
                } else {
                    entry.invalid = true;
                }
            }
            "backspace" | "delete" => {
                if entry.replace {
                    entry.text.clear();
                } else {
                    entry.text.pop();
                }
                entry.replace = false;
                entry.invalid = false;
            }
            "a" if key.modifiers.platform => entry.replace = true,
            "v" if key.modifiers.platform => {
                if let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) {
                    entry.append(&text);
                }
            }
            "tab" => {
                self.entry = None;
                return false;
            }
            _ if !key.modifiers.platform && !key.modifiers.control => {
                if let Some(text) = key
                    .key_char
                    .as_deref()
                    .or(Some(key.key.as_str()))
                    .filter(|text| text.chars().count() == 1)
                {
                    entry.append(text);
                }
            }
            _ => (),
        }
        cx.stop_propagation();
        cx.notify();
        true
    }

    fn gain_display(&self, values: &Values) -> f32 {
        const SCALES: [f32; 6] = [0.0, 6.0, 12.0, 24.0, 36.0, 60.0];
        if self.gain_scale > 0 {
            return SCALES[self.gain_scale];
        }
        let extent = members(selection::present(values))
            .map(|band| {
                let makeup = P::Makeup.id(band);
                let db = parameters::spec(makeup).plain(values[makeup]);
                db.abs().max((db + selection::range_db(values, band)).abs()) as f32
            })
            .fold(0.0, f32::max);
        SCALES[2..]
            .iter()
            .copied()
            .find(|scale| *scale >= extent * 1.12)
            .unwrap_or(60.0)
    }

    fn chart_view(&self, values: &Values) -> plot::Viewport {
        self.viewport.gain(self.chart_gain(values))
    }

    fn chart_gain(&self, values: &Values) -> f32 {
        match self.drag {
            Some(Drag::Vertical { .. } | Drag::Center { .. } | Drag::Range { .. }) => {
                self.drag_gain_db
            }
            _ => self.gain_display(values),
        }
    }

    fn gain_button(&self, values: &Values, cx: &mut Context<Self>) -> AnyElement {
        let focus = self.gain_focus.clone();
        let display = self.chart_gain(values);
        let text = if self.gain_scale == 0 {
            format!("Gain Auto ±{display:.0}")
        } else {
            format!("Gain ±{display:.0}")
        };
        div()
            .id("gain-display")
            .track_focus(&focus)
            .tab_stop(true)
            .bg(rgb(PAPER))
            .text_size(px(11.0))
            .px(px(6.0))
            .py(px(3.0))
            .border_1()
            .border_color(rgb(HAIR))
            .rounded(px(4.0))
            .focus(|style| style.border_color(rgb(ACCENT)))
            .cursor_pointer()
            .child(text)
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, event: &MouseDownEvent, window, cx| {
                    window.focus(&focus, cx);
                    let step = if event.modifiers.shift { 5 } else { 1 };
                    this.gain_scale = (this.gain_scale + step) % 6;
                    cx.notify();
                }),
            )
            .on_key_down(cx.listener(|this, event: &gpui::KeyDownEvent, _, cx| {
                let step = match event.keystroke.key.as_str() {
                    "space" | "enter" | "right" | "up" => 1,
                    "left" | "down" => 5,
                    _ => return,
                };
                this.gain_scale = (this.gain_scale + step) % 6;
                cx.stop_propagation();
                cx.notify();
            }))
            .into_any_element()
    }

    fn move_drag(&mut self, event: &MouseMoveEvent, _: &mut Window, cx: &mut Context<Self>) {
        let bounds = self.plot.get();
        let over_controls = bounds.is_some_and(|bounds| {
            self.panel
                .is_some_and(|panel| panel.contains(bounds, event.position))
                || self.threshold_marker.is_some_and(|marker| {
                    marker.grip.contains(&event.position) || marker.hits_line(event.position)
                })
        });
        let hover = bounds
            .filter(|bounds| {
                !over_controls
                    && bounds.contains(&event.position)
                    && f32::from(event.position.y - bounds.top()) < plot::ruler(*bounds) - 15.0
            })
            .map(|bounds| plot::at_x(bounds, self.viewport, event.position.x));
        let hovered = bounds.and_then(|bounds| {
            if over_controls || !bounds.contains(&event.position) {
                return None;
            }
            let values = self.params.values();
            let view = self.chart_view(&values);
            if let Some(band) = self.hovered_band.filter(|_| self.selection == 0) {
                if controls::hover_strip(bounds, view, &values, band)
                    .is_some_and(|strip| strip.dilate(px(13.0)).contains(&event.position))
                {
                    return Some(band);
                }
            }
            plot::hit(bounds, view, &values, self.selected, event.position).map(plot::Hit::band)
        });
        if hover != self.hover || hovered != self.hovered_band {
            self.hover = hover;
            self.hovered_band = hovered;
            cx.notify();
        }
        let Some(drag) = self.drag else {
            return;
        };
        if event.pressed_button != Some(MouseButton::Left) {
            self.release(cx);
            return;
        }
        if let Drag::Ruler {
            start,
            initial,
            anchor,
        } = drag
        {
            if let Some(bounds) = bounds {
                let dx = f64::from(f32::from(start.x - event.position.x));
                let dy = f64::from(f32::from(event.position.y - start.y));
                let width = f64::from(f32::from(bounds.size.width) - plot::PAD * 2.0);
                self.viewport = initial.move_from(anchor, (dy * 0.010).exp(), dx / width.max(1.0));
            }
            cx.notify();
            return;
        }
        if let Drag::Marquee { start, .. } = drag {
            self.drag = Some(Drag::Marquee {
                start,
                end: event.position,
            });
            if let Some(bounds) = bounds {
                self.selection = plot::enclosed(
                    bounds,
                    self.viewport,
                    &self.params.values(),
                    start,
                    event.position,
                );
                if let Some(band) = members(self.selection).next() {
                    self.selected = band;
                }
            }
            cx.notify();
            return;
        }
        if self.editing.borrow().is_empty() {
            self.drag = None;
            return;
        }
        let fine = if event.modifiers.shift { 0.125 } else { 1.0 };
        match drag {
            Drag::Vertical {
                start,
                value,
                drag_pixels,
                boundary,
            } => {
                let distance = f64::from(f32::from(start - event.position.y));
                let delta =
                    (value + distance * fine / f64::from(drag_pixels)).clamp(0.0, 1.0) - value;
                if let Some(boundary) = boundary {
                    for (id, value) in boundary.edits(value + delta) {
                        self.change(id, value);
                    }
                } else {
                    for target in self.editing.borrow().iter().copied() {
                        self.change(target, self.drag_initial[target] + delta);
                    }
                }
            }
            Drag::Detector {
                start,
                edge,
                horizontal,
                moved,
            } => {
                let (pixels, delta) = if horizontal {
                    let Some(bounds) = bounds else { return };
                    let delta = plot::at_x(bounds, self.viewport, event.position.x)
                        - plot::at_x(bounds, self.viewport, start.x);
                    (event.position.x - start.x, delta)
                } else {
                    let distance = start.y - event.position.y;
                    (distance, f64::from(f32::from(distance)) / 150.0)
                };
                if moved || pixels.abs() > px(2.0) {
                    self.drag = Some(Drag::Detector {
                        start,
                        edge,
                        horizontal,
                        moved: true,
                    });
                    with_group_edit(&self.handler, edge.grouped(), || {
                        for (id, value) in edge.edits(edge.value() + delta * fine) {
                            self.change(id, value);
                        }
                    });
                }
            }
            Drag::Edge { start, boundary } => {
                let Some(bounds) = bounds else { return };
                let delta = (plot::at_x(bounds, self.viewport, event.position.x) - start) * fine;
                for (id, value) in boundary.edits(boundary.value + delta) {
                    self.change(id, value);
                }
            }
            Drag::Center { start, group, axis } => {
                let Some(bounds) = bounds else { return };
                let dx = f32::from(event.position.x - start.x);
                let dy = f32::from(start.y - event.position.y);
                let axis = axis.or_else(|| {
                    if dx.abs().max(dy.abs()) < 2.0 {
                        None
                    } else if dx.abs() >= dy.abs() {
                        Some(Axis::Frequency)
                    } else {
                        Some(Axis::Gain)
                    }
                });
                self.drag = Some(Drag::Center { start, group, axis });
                let delta = (plot::at_x(bounds, self.viewport, event.position.x)
                    - plot::at_x(bounds, self.viewport, start.x))
                    * fine;
                if event.modifiers.platform {
                    let layout = Layout::new(&self.drag_initial);
                    let [left, right] =
                        [0, 1].map(|side| layout.boundary(&self.drag_initial, self.selected, side));
                    let max_shrink =
                        ((right.value - left.value) - parameters::layout::MIN_WIDTH) * 0.5;
                    let widen = (f64::from(dy) * fine / 450.0).max(-max_shrink.max(0.0));
                    for (boundary, value) in
                        [(left, left.value - widen), (right, right.value + widen)]
                    {
                        if boundary.movable() {
                            for (id, value) in boundary.edits(value) {
                                self.change(id, value);
                            }
                        }
                    }
                } else {
                    if axis == Some(Axis::Frequency) || event.modifiers.alt {
                        let delta = selection::group_delta(&self.drag_edges, delta);
                        for &boundary in &self.drag_edges {
                            for (id, value) in boundary.edits(boundary.value + delta) {
                                self.change(id, value);
                            }
                        }
                    }
                    if axis == Some(Axis::Gain) || event.modifiers.alt {
                        for band in members(group) {
                            let gain = P::Makeup.id(band);
                            self.change(
                                gain,
                                self.drag_initial[gain]
                                    + f64::from(dy / self.drag_db_pixels) * fine / 36.0,
                            );
                        }
                    }
                }
            }
            Drag::Range {
                start,
                group,
                db_per_pixel,
            } => {
                let delta = f64::from(f32::from(start - event.position.y)) * fine * db_per_pixel;
                for band in members(group) {
                    let value = selection::range_db(&self.drag_initial, band) + delta;
                    for (id, value) in selection::range_edits(&self.drag_initial, band, value) {
                        self.change(id, value);
                    }
                }
            }
            Drag::Marquee { .. } | Drag::Ruler { .. } => (),
        }
        cx.notify();
    }

    fn release(&mut self, cx: &mut Context<Self>) {
        self.drag = None;
        controls::restore_press(&self.press, &self.params, &self.handler, false);
        self.finish_edit();
        cx.notify();
    }

    fn chart_key(
        &mut self,
        event: &gpui::KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let key = &event.keystroke;
        let values = self.params.values();
        let layout = Layout::new(&values);
        let order = layout.order();
        match key.key.as_str() {
            "a" if key.modifiers.platform => {
                self.selection = selection::present(&values);
            }
            #[cfg(target_os = "macos")]
            "escape" if self.presentation.is_fullscreen() => self.toggle_screen(window, cx),
            "escape" if self.selection != 0 => {
                self.run_command(Command::Deselect, window, cx);
            }
            "left" | "right" if !order.is_empty() => {
                let current = order
                    .iter()
                    .position(|band| self.selection != 0 && *band == self.selected);
                let index = match current {
                    None => {
                        if key.key == "left" {
                            order.len() - 1
                        } else {
                            0
                        }
                    }
                    Some(index) if key.key == "left" => index.saturating_sub(1),
                    Some(index) => (index + 1).min(order.len() - 1),
                };
                if !key.modifiers.shift {
                    self.selection = 0;
                }
                self.select(order[index], key.modifiers, &values);
            }
            "enter" => {
                let hover = self.hover.take();
                self.add_at_cursor(window, cx);
                self.hover = hover;
                window.focus(&self.chart_focus, cx);
            }
            "f2" if self.selection != 0 => {
                self.begin_number(P::Makeup.id(self.selected), window, cx)
            }
            "s" if self.selection != 0 => {
                self.button_toggle(P::Solo.id(self.selected), key.modifiers.platform)
            }
            "m" if self.selection != 0 => {
                self.button_toggle(P::Mute.id(self.selected), key.modifiers.platform)
            }
            "d" if self.selection != 0 && !key.modifiers.platform && !key.modifiers.control => {
                self.detector_open = !self.detector_open;
            }
            "k" if self.selection != 0 && !key.modifiers.platform && !key.modifiers.control => {
                self.button_toggle(P::Audition.id(self.selected), false);
            }
            "0" => self.viewport = plot::Viewport::default(),
            _ => return,
        }
        cx.stop_propagation();
        cx.notify();
    }

    fn chart_down(&mut self, event: &MouseDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        self.entry = None;
        let Some(bounds) = self.plot.get() else {
            return;
        };
        window.focus(&self.chart_focus, cx);
        let mut values = self.params.values();
        if let Some(range) = self.detector_range(&values) {
            if let Some(side) = range.hit(bounds, self.viewport, event.position) {
                let edge = range.edge(side);
                if event.click_count == 2 {
                    self.begin_number(edge.id(), window, cx);
                } else {
                    self.start_detector(event.position, edge, true);
                    cx.notify();
                }
                return;
            }
        }
        let y = f32::from(event.position.y - bounds.top());
        let ruler = plot::ruler(bounds);
        if (ruler - 14.0..ruler + 5.0).contains(&y) {
            if event.click_count == 2 {
                self.viewport = plot::Viewport::default();
            } else {
                self.drag = Some(Drag::Ruler {
                    start: event.position,
                    initial: self.viewport,
                    anchor: plot::at_x(bounds, self.viewport, event.position.x),
                });
            }
            cx.notify();
            return;
        }
        if y >= ruler - 14.0 {
            return;
        }
        let x = plot::at_x(bounds, self.viewport, event.position.x);
        let mut hit = plot::hit(
            bounds,
            self.chart_view(&values),
            &values,
            self.selected,
            event.position,
        );
        if let Some(marker) = self.threshold_marker.filter(|marker| {
            matches!(hit, None | Some(plot::Hit::Body(_))) && marker.hits_line(event.position)
        }) {
            self.threshold_down(marker, event, window, cx);
            return;
        }
        let empty = selection::present(&values) == 0;
        let mut created = false;
        let add = y <= 29.0 && self.hover.is_some() && hit.is_none();
        if add || (hit.is_none() && (event.click_count == 2 || empty)) {
            if let Some(band) = self.create_band(x) {
                values = self.params.values();
                hit = Some(plot::Hit::Center(band));
                created = true;
            }
        }
        let Some(hit) = hit else {
            self.selection = 0;
            self.drag = Some(Drag::Marquee {
                start: event.position,
                end: event.position,
            });
            cx.notify();
            return;
        };
        let band = hit.band();
        let resize = event.modifiers.platform
            && matches!(hit, plot::Hit::Center(_))
            && self.selection & one(band) != 0;
        if !resize {
            self.select(band, event.modifiers, &values);
        }
        if (event.modifiers.platform && !resize)
            || (event.modifiers.shift && self.selection & one(band) == 0)
        {
            cx.notify();
            return;
        }
        if !created && event.click_count == 2 {
            let id = match hit {
                plot::Hit::Edge(_, side) => {
                    Layout::new(&values).boundary(&values, band, side).ids[0]
                }
                plot::Hit::Center(_) => Some(P::Makeup.id(band)),
                plot::Hit::Range(_) => Some(P::Range.id(band)),
                plot::Hit::Body(_) => None,
            };
            if let Some(id) = id {
                self.begin_number(id, window, cx);
                return;
            }
        }
        match hit {
            plot::Hit::Edge(_, side) => {
                let boundary = Layout::new(&values).boundary(&values, band, side);
                if boundary.movable() {
                    self.begin(boundary.ids.into_iter().flatten());
                    self.drag = Some(Drag::Edge { start: x, boundary });
                }
            }
            plot::Hit::Center(_) => {
                self.drag_db_pixels = self.chart_view(&values).db_pixels(bounds);
                self.drag_gain_db = self.gain_display(&values);
                self.drag_initial = values;
                self.drag_edges = selection::boundaries(&values, self.selection);
                self.begin(
                    self.drag_edges
                        .iter()
                        .flat_map(|edge| edge.ids.into_iter().flatten())
                        .chain(members(self.selection).map(|band| P::Makeup.id(band))),
                );
                self.drag = Some(Drag::Center {
                    start: event.position,
                    group: self.selection,
                    axis: None,
                });
            }
            plot::Hit::Range(_) => {
                let view = self.chart_view(&values);
                let weight = plot::range_drag_weight(bounds, view, &values, band, event.position.x);
                self.start_range(
                    band,
                    event.position.y,
                    f64::from(view.db_pixels(bounds).recip()) / weight,
                );
            }
            plot::Hit::Body(_) => (),
        }
        cx.notify();
    }

    fn chart_scroll(
        &mut self,
        event: &gpui::ScrollWheelEvent,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(bounds) = self.plot.get() else {
            return;
        };
        let y = f32::from(event.position.y - bounds.top());
        let ruler = plot::ruler(bounds);
        let delta = f32::from(event.delta.pixel_delta(px(16.0)).y);
        if delta.abs() < 0.1 {
            return;
        }
        if (ruler - 14.0..ruler + 5.0).contains(&y) {
            let anchor = plot::at_x(bounds, self.viewport, event.position.x);
            self.viewport = self
                .viewport
                .move_from(anchor, f64::from(-delta * 0.008).exp(), 0.0);
            cx.stop_propagation();
            cx.notify();
            return;
        }
        if !(0.0..ruler - 14.0).contains(&y) {
            return;
        }
        let values = self.params.values();
        match plot::hit(
            bounds,
            self.chart_view(&values),
            &values,
            self.selected,
            event.position,
        ) {
            Some(plot::Hit::Edge(band, side)) => {
                let ids = plot::edge_ids(&values, band);
                let low_is_first = ids[0] == P::Low.id(band);
                let id = if low_is_first == (side == 0) {
                    P::LowSlope
                } else {
                    P::HighSlope
                }
                .id(band);
                let spec = parameters::spec(id);
                let next = spec.plain(values[id]) + f64::from(delta.signum());
                self.edit(id, spec.normalized(next));
            }
            Some(hit) => {
                let band = hit.band();
                let layout = Layout::new(&values);
                let [left, right] = [0, 1].map(|side| layout.boundary(&values, band, side));
                let width = right.value - left.value;
                let delta = (width * (f64::from(delta * 0.003).exp() - 1.0) * 0.5)
                    .max(-(width - parameters::layout::MIN_WIDTH).max(0.0) * 0.5);
                self.edit_many(
                    [(left, left.value - delta), (right, right.value + delta)]
                        .into_iter()
                        .filter(|(edge, _)| edge.movable())
                        .flat_map(|(edge, value)| edge.edits(value)),
                );
            }
            None => return,
        }
        cx.stop_propagation();
        cx.notify();
    }

    fn chart_solo(&mut self, event: &MouseDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let Some(bounds) = self.plot.get() else {
            return;
        };
        let Some(hit) = plot::hit(
            bounds,
            self.chart_view(&self.params.values()),
            &self.params.values(),
            self.selected,
            event.position,
        ) else {
            return;
        };
        let band = hit.band();
        self.selected = band;
        self.selection = one(band);
        window.focus(&self.chart_focus, cx);
        let field = if event.modifiers.alt {
            P::Mute
        } else {
            P::Solo
        };
        self.button_toggle(field.id(band), event.modifiers.platform);
        cx.notify();
    }

    fn footer_value(&self, id: usize, cx: &mut Context<Self>) -> AnyElement {
        self.value(id, ValueStyle::Footer, cx)
    }

    fn start_vertical(&mut self, id: usize, value: f64, start: Pixels, drag_pixels: f32) {
        self.drag_initial = self.params.values();
        self.drag_gain_db = self.gain_display(&self.drag_initial);
        let boundary = self.boundary(id);
        if let Some(edge) = boundary {
            if !edge.movable() {
                return;
            }
            self.begin(edge.ids.into_iter().flatten());
        } else {
            self.begin(self.control_ids(id));
        }
        self.drag = Some(Drag::Vertical {
            start,
            value,
            drag_pixels,
            boundary,
        });
    }

    fn threshold_down(
        &mut self,
        marker: threshold::Marker,
        event: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let id = P::Threshold.id(self.selected);
        window.focus(&self.threshold_focus, cx);
        self.entry = None;
        if event.modifiers.alt {
            self.reset_control(id);
        } else if event.click_count == 2 {
            self.prepare_number(id);
        } else {
            self.start_vertical(
                id,
                self.params.get(id),
                event.position.y,
                marker.drag_pixels,
            );
        }
        cx.notify();
    }

    fn value(&self, id: usize, style: ValueStyle, cx: &mut Context<Self>) -> AnyElement {
        let compact = !matches!(style, ValueStyle::Footer);
        let threshold = match style {
            ValueStyle::Threshold(marker, detector) => Some((marker, detector)),
            _ => None,
        };
        let marker = threshold.map(|(marker, _)| marker);
        let spec = parameters::spec(id);
        let values = self.params.values();
        let value = self.control_value(&values, id);
        let source = (compact && P::Key.id(self.selected) == id)
            .then(|| Source::read(&values, self.selected));
        let focus = if marker.is_some() {
            &self.threshold_focus
        } else {
            &self.focus[id]
        }
        .clone();
        let discrete = !spec.choices.is_empty();
        let entry = self.entry.as_ref().filter(|entry| entry.id == id);
        let signed =
            selection::range_band(id).map(|band| selection::range_db(&self.params.values(), band));
        let display = entry.map_or_else(
            || {
                if let Some(source) = source {
                    source.title().to_owned()
                } else {
                    signed.map_or_else(|| spec.display(value), |db| format!("{db:+.1} dB"))
                }
            },
            |entry| format!("{}{}", entry.text, if entry.replace { "" } else { "▏" }),
        );
        let key_meter = (0..BANDS)
            .find(|&band| P::Threshold.id(band) == id)
            .map(|band| spec.normalized(f64::from(self.detector_level(band))) as f32);
        let border = if entry.is_some() {
            ACCENT
        } else if compact {
            BG
        } else {
            HAIR
        };
        let interactive = div()
            .id((
                if marker.is_some() {
                    "threshold-marker"
                } else {
                    "param"
                },
                id,
            ))
            .track_focus(&focus)
            .tab_stop(true)
            .flex_1()
            .min_w_0()
            .rounded(px(5.0))
            .border_1()
            .border_color(rgb(border))
            .focus(|style| style.border_color(rgb(ACCENT)))
            .hover(|style| style.bg(rgb(0x292929)))
            .bg(rgb(if compact { BG } else { PAPER }))
            .cursor_pointer()
            .px(px(if compact { 4.0 } else { 6.0 }))
            .py(px(if compact { 2.0 } else { 0.0 }))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, event: &MouseDownEvent, window, cx| {
                    if compact {
                        cx.stop_propagation();
                    }
                    if let Some(marker) = marker {
                        this.threshold_down(marker, event, window, cx);
                        return;
                    }
                    window.focus(&focus, cx);
                    this.entry = None;
                    if event.modifiers.alt && !discrete {
                        this.reset_control(id);
                    } else if discrete {
                        this.control_step(id, if event.modifiers.shift { -1 } else { 1 }, false);
                    } else if event.click_count == 2 {
                        this.prepare_number(id);
                    } else if let Some(edge) = key::edge_for_id(&this.params.values(), id) {
                        this.start_detector(event.position, edge, false);
                    } else if let Some(band) = selection::range_band(id) {
                        this.start_range(band, event.position.y, 0.24);
                    } else {
                        this.start_vertical(id, value, event.position.y, VERTICAL_DRAG_PIXELS);
                    }
                    cx.notify();
                }),
            )
            .on_key_down(cx.listener(move |this, event: &gpui::KeyDownEvent, _, cx| {
                if this.entry_key(id, &event.keystroke, cx) {
                    return;
                }
                match event.keystroke.key.as_str() {
                    "enter" | "f2" if !discrete => this.prepare_number(id),
                    "up" | "right" => this.control_step(id, 1, event.keystroke.modifiers.shift),
                    "down" | "left" => this.control_step(id, -1, event.keystroke.modifiers.shift),
                    "home" if !discrete => this.reset_control(id),
                    "enter" | "space" if discrete => this.control_step(id, 1, false),
                    _ => return,
                }
                cx.stop_propagation();
                cx.notify();
            }));
        if let Some((marker, detector)) = threshold {
            let mut slider = interactive;
            for (action, direction) in [
                (gpui::AccessibleAction::Increment, 1),
                (gpui::AccessibleAction::Decrement, -1),
            ] {
                let editor = cx.entity().downgrade();
                slider = slider.on_a11y_action(action, move |_, _, cx| {
                    editor
                        .update(cx, |this, cx| {
                            this.control_step(id, direction, false);
                            cx.notify();
                        })
                        .ok();
                });
            }
            return marker.control(slider, spec, value, entry, display, detector);
        }
        let label = div()
            .overflow_hidden()
            .whitespace_nowrap()
            .text_size(px(10.0))
            .line_height(px(13.0))
            .text_color(rgb(MUTED))
            .child(if source.is_some() {
                "Source"
            } else {
                spec.name
            });
        let readout = div()
            .overflow_hidden()
            .whitespace_nowrap()
            .text_size(px(if matches!(style, ValueStyle::Knob) {
                13.0
            } else {
                11.0
            }))
            .line_height(px(16.0))
            .text_color(rgb(if entry.is_some_and(|entry| entry.invalid) {
                ACCENT
            } else {
                TEXT
            }))
            .child(display);
        if compact {
            if matches!(style, ValueStyle::Compact) {
                return interactive
                    .flex()
                    .flex_col()
                    .justify_center()
                    .child(label)
                    .child(readout)
                    .into_any_element();
            }
            let zero = if signed.is_some() || P::Makeup.id(self.selected) == id {
                0.5
            } else {
                0.0
            };
            let position = signed.map_or(value as f32, knob::signed_position);
            return interactive
                .flex()
                .flex_col()
                .items_center()
                .justify_center()
                .gap(px(2.0))
                .child(knob::dial(position, zero, key_meter))
                .child(div().text_center().child(label).child(readout))
                .into_any_element();
        }
        interactive
            .h(px(BAR_CONTROL_HEIGHT))
            .flex()
            .items_center()
            .gap(px(6.0))
            .child(label.flex_none())
            .child(readout.flex_1().min_w_0().text_right())
            .into_any_element()
    }

    fn band_button(
        &self,
        id: usize,
        name: &'static str,
        accent: u32,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        self.switch_button(id, name, accent, None, cx)
    }

    fn switch_button(
        &self,
        id: usize,
        name: &'static str,
        accent: u32,
        graph: Option<usize>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let active = self.params.get(id) > 0.5;
        let focus = graph
            .map_or(&self.focus[id], |index| &self.graph_focus[index])
            .clone();
        let button = div()
            .id((
                if graph.is_some() {
                    "graph-switch"
                } else {
                    "switch"
                },
                id,
            ))
            .rounded(px(4.0))
            .border_1()
            .border_color(rgb(if active { accent } else { HAIR }))
            .focus(|style| style.border_color(rgb(RAIL_TEXT)))
            .cursor_pointer()
            .bg(rgb(if active { accent } else { RAIL }))
            .text_size(px(10.0))
            .text_color(rgb(if active { 0x101010 } else { RAIL_MUTED }))
            .px(px(if graph.is_some() { 4.0 } else { 7.0 }))
            .py(px(if graph.is_some() { 0.0 } else { 2.0 }))
            .child(name);
        self.switch_control(id, focus, button, cx)
    }

    fn switch_control(
        &self,
        id: usize,
        focus: FocusHandle,
        button: gpui::Stateful<gpui::Div>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        button
            .track_focus(&focus)
            .tab_stop(true)
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, event: &MouseDownEvent, window, cx| {
                    window.focus(&focus, cx);
                    this.button_press(id, event.modifiers.platform);
                    cx.stop_propagation();
                    cx.notify();
                }),
            )
            .on_key_down(cx.listener(move |this, event: &gpui::KeyDownEvent, _, cx| {
                if matches!(event.keystroke.key.as_str(), "space" | "enter") {
                    this.button_toggle(id, event.keystroke.modifiers.platform);
                    cx.stop_propagation();
                    cx.notify();
                }
            }))
            .into_any_element()
    }

    fn band_row(
        &self,
        band: usize,
        values: &parameters::Values,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let selected = self.selection & one(band) != 0;
        let focus = self.band_focus[band].clone();
        let color = ACCENT;
        let db = self.activity[band];
        let enabled = values[P::Enabled.id(band)] > 0.5;
        let gain = if enabled {
            format!("{db:+.1}")
        } else {
            "—".to_owned()
        };
        div()
            .flex()
            .flex_col()
            .bg(rgb(if selected { 0x2a2a2a } else { RAIL }))
            .rounded(px(5.0))
            .px(px(9.0))
            .pt(px(6.0))
            .pb(px(8.0))
            .gap(px(5.0))
            .child(
                div()
                    .id(("band", band))
                    .track_focus(&focus)
                    .tab_stop(true)
                    .border_1()
                    .border_color(rgb(if selected { 0x2a2a2a } else { RAIL }))
                    .focus(|style| style.border_color(rgb(RAIL_MUTED)))
                    .cursor_pointer()
                    .flex()
                    .justify_between()
                    .items_center()
                    .on_click(
                        cx.listener(move |this, event: &gpui::ClickEvent, window, cx| {
                            this.select(band, event.modifiers(), &this.params.values());
                            window.focus(&focus, cx);
                            cx.notify();
                        }),
                    )
                    .on_key_down(cx.listener(
                        move |this, event: &gpui::KeyDownEvent, window, cx| {
                            let next = match event.keystroke.key.as_str() {
                                "up" | "down" => {
                                    let layout = Layout::new(&this.params.values());
                                    let order = layout.order();
                                    let index =
                                        order.iter().position(|other| *other == band).unwrap_or(0);
                                    let target = if event.keystroke.key == "up" {
                                        index.saturating_sub(1)
                                    } else {
                                        index + 1
                                    };
                                    order.get(target).copied().unwrap_or(band)
                                }
                                "enter" | "space" => band,
                                _ => return,
                            };
                            this.selected = next;
                            this.selection = one(next);
                            window.focus(&this.band_focus[next], cx);
                            cx.notify();
                        },
                    ))
                    .child(
                        div()
                            .text_size(px(13.0))
                            .text_color(rgb(RAIL_TEXT))
                            .child(format!("Band {}", band + 1)),
                    )
                    .child(
                        div()
                            .text_size(px(11.0))
                            .text_color(rgb(RAIL_MUTED))
                            .child(gain),
                    ),
            )
            .child(
                div()
                    .text_size(px(10.0))
                    .text_color(rgb(RAIL_MUTED))
                    .child(plot::range_text(values, band)),
            )
            .child(
                div()
                    .flex()
                    .gap(px(5.0))
                    .child(self.band_button(
                        P::Enabled.id(band),
                        if enabled { "On" } else { "Off" },
                        color,
                        cx,
                    ))
                    .child(self.band_button(P::Solo.id(band), "Solo", color, cx))
                    .child(self.band_button(P::Mute.id(band), "Mute", color, cx)),
            )
            .child(
                div().h(px(2.0)).bg(rgb(0x4a4a4a)).child(
                    div()
                        .h_full()
                        .w(relative((db.abs() / 24.0).clamp(0.0, 1.0)))
                        .bg(rgb(ACCENT)),
                ),
            )
            .into_any_element()
    }

    fn toggle_trace(&mut self, index: usize) {
        if index == TRACES {
            self.freeze = !self.freeze;
        } else {
            self.traces[index] = !self.traces[index];
        }
        self.analyzer.invalidate();
    }

    fn trace_switch(&self, index: usize, name: &'static str, cx: &mut Context<Self>) -> AnyElement {
        let active = if index == TRACES {
            self.freeze
        } else {
            self.traces[index]
        };
        let focus = self.trace_focus[index].clone();
        div()
            .id(("trace", index))
            .track_focus(&focus)
            .tab_stop(true)
            .rounded(px(4.0))
            .border_1()
            .border_color(rgb(HAIR))
            .focus(|style| style.border_color(rgb(ACCENT)))
            .bg(rgb(if active { 0x373737 } else { PAPER }))
            .px(px(8.0))
            .py(px(3.0))
            .cursor_pointer()
            .text_size(px(11.0))
            .child(name)
            .on_click(cx.listener(move |this, _, window, cx| {
                window.focus(&focus, cx);
                this.toggle_trace(index);
                cx.notify();
            }))
            .on_key_down(cx.listener(move |this, event: &gpui::KeyDownEvent, _, cx| {
                if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                    this.toggle_trace(index);
                    cx.notify();
                }
            }))
            .into_any_element()
    }
}

impl Editor {
    fn run_command(&mut self, command: Command, window: &mut Window, cx: &mut Context<Self>) {
        match command {
            Command::Remove => self.remove_selected(window, cx),
            Command::Split => self.add_at_cursor(window, cx),
            Command::Undo => self.history_step(false, cx),
            Command::Redo => self.history_step(true, cx),
            Command::Deselect => {
                self.entry = None;
                self.selection = 0;
                self.hovered_band = None;
                window.focus(&self.chart_focus, cx);
                cx.notify();
            }
        }
    }

    fn chart_button(&self, command: Command, cx: &mut Context<Self>) -> AnyElement {
        let label = command.label();
        let enabled = match command {
            Command::Undo => self.journal().borrow().available(false),
            Command::Redo => self.journal().borrow().available(true),
            _ => true,
        };
        let focus = self.command_focus[command as usize].clone();
        let button = match command {
            Command::Remove => icon::Icon::Remove.button(label, false),
            Command::Deselect => icon::Icon::Close.button(label, false),
            _ => div()
                .id(label)
                .focus(|style| style.border_color(rgb(ACCENT)))
                .border_1()
                .border_color(rgb(HAIR))
                .rounded(px(4.0))
                .bg(rgb(PAPER))
                .hover(|style| style.bg(rgb(0x393939)))
                .text_color(rgb(if enabled { TEXT } else { 0x777777 }))
                .text_size(px(11.0))
                .px(px(8.0))
                .py(px(3.0))
                .cursor_pointer()
                .child(label),
        };
        button
            .track_focus(&focus)
            .tab_stop(enabled)
            .on_click(cx.listener(move |this, _, window, cx| {
                if enabled {
                    window.focus(&focus, cx);
                    this.run_command(command, window, cx);
                }
            }))
            .on_key_down(
                cx.listener(move |this, event: &gpui::KeyDownEvent, window, cx| {
                    if enabled && matches!(event.keystroke.key.as_str(), "enter" | "space") {
                        this.run_command(command, window, cx);
                        cx.stop_propagation();
                    }
                }),
            )
            .into_any_element()
    }
}

impl Render for Editor {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let values = self.params.values();
        let width = view::dimensions(window).0;
        let rail_width = if width < 1050 { 166.0 } else { 213.0 };
        let created = selection::present(&values);
        self.selection &= created;
        if self.selection & one(self.selected) == 0 {
            self.selected = members(self.selection).next().unwrap_or(0);
        }
        let selected = (self.selection != 0).then_some(self.selected);
        let selection = self.selection;
        let can_add = created.count_ones() < BANDS as u32;
        let layout = Layout::new(&values);
        let view = self.chart_view(&values);
        let hover = self.hover.filter(|&x| {
            can_add
                && self.drag.is_none()
                && self.hovered_band.is_none()
                && (created == 0 || layout.at(x).is_some())
        });
        let draft = hover;
        let graph_controls = self
            .hovered_band
            .filter(|&band| {
                selected.is_none() && self.drag.is_none() && parameters::present(&values, band)
            })
            .and_then(|band| self.graph_controls(band, &values, view, cx));
        let marquee = match self.drag {
            Some(Drag::Marquee { start, end }) => Some([start, end]),
            _ => None,
        };
        let detector = self.detector_range(&values);
        let detector_edge = match self.drag {
            Some(Drag::Detector { edge, .. }) => {
                Some(usize::from(edge.id() == P::KeyHigh.id(self.selected)))
            }
            _ => None,
        };
        let spectra = self.spectra;
        let traces = self.traces;
        let activity = self.activity;
        let bounds_slot = Rc::clone(&self.plot);
        let spectrum = canvas(
            move |bounds, _, _| bounds_slot.set(Some(bounds)),
            move |bounds, _, window, _| {
                plot::draw(
                    window,
                    bounds,
                    plot::Display {
                        view,
                        values: &values,
                        selected,
                        selection,
                        draft,
                        marquee,
                        detector,
                        detector_edge,
                        spectra: &spectra,
                        shown: traces,
                        activity,
                    },
                )
            },
        )
        .size_full();
        let inspector = selected.and_then(|band| self.band_inspector(band, &values, view, cx));
        if selected.is_none() {
            self.panel = None;
        }
        let threshold = selected.and_then(|band| {
            let chart = self.plot.get()?;
            let id = P::Threshold.id(band);
            let marker = threshold::Marker::new(
                chart,
                view,
                layout.edges[band],
                parameters::spec(id),
                values[id],
                self.panel?.bounds(chart),
            )?;
            let detector = threshold::DetectorReading::new(
                self.detector_level(band),
                activity[band],
                values[P::Enabled.id(band)] > 0.5 && values[BYPASS] <= 0.5,
            );
            Some((id, marker, detector))
        });
        self.threshold_marker = threshold.map(|(_, marker, _)| marker);
        let threshold_focused = self.threshold_focus.is_focused(window);
        let threshold_control = threshold.map(|(id, marker, detector)| {
            self.value(id, ValueStyle::Threshold(marker, detector), cx)
        });
        let guidance = if threshold_focused {
            "Drag the threshold line or handle · Shift for fine adjustment · Enter to type"
        } else if detector.is_some() {
            "Drag the KEY grips to set the detector range · Shift for fine adjustment · K audition"
        } else if self.chart_focus.is_focused(window) {
            "←/→ select  ·  Enter split  ·  D detector  ·  S solo  ·  M mute  ·  0 reset zoom"
        } else if created == 0 {
            "Click the spectrum to start with one full-range band"
        } else if self.selection == 0 {
            "Hover over a frequency and click + to split its band · Drag a box to select bands"
        } else {
            "Drag a crossover to adjust both bands · Scroll over an edge to set its slope"
        };

        let presets = self.preset_strip(&values, cx);
        let preset_overlay = self.browser.open.then(|| self.preset_overlay(&values, cx));
        div()
            .id("mbx")
            .relative()
            .size_full()
            .bg(rgb(BG))
            .text_color(rgb(TEXT))
            .flex()
            .flex_col()
            .on_mouse_move(cx.listener(Self::move_drag))
            .on_key_down(cx.listener(|this, event: &gpui::KeyDownEvent, window, cx| {
                #[cfg(target_os = "macos")]
                if event.keystroke.key == "escape" && this.presentation.is_fullscreen() {
                    this.toggle_screen(window, cx);
                    cx.stop_propagation();
                    return;
                }
                if event.keystroke.modifiers.platform && event.keystroke.key == "z" {
                    this.history_step(event.keystroke.modifiers.shift, cx);
                    cx.stop_propagation();
                } else if matches!(event.keystroke.key.as_str(), "backspace" | "delete")
                    && this.selection != 0
                {
                    this.remove_selected(window, cx);
                    cx.stop_propagation();
                }
            }))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _: &MouseUpEvent, _, cx| this.release(cx)),
            )
            .on_mouse_up_out(
                MouseButton::Left,
                cx.listener(|this, _: &MouseUpEvent, _, cx| this.release(cx)),
            )
            .child(
                div()
                    .h(px(HEADER_HEIGHT))
                    .flex_none()
                    .bg(rgb(PAPER))
                    .px(px(HEADER_PADDING))
                    .border_b_1()
                    .border_color(rgb(HAIR))
                    .flex()
                    .items_center()
                    .gap(px(HEADER_GAP))
                    .child(
                        div()
                            .w(px(TITLE_WIDTH))
                            .flex_none()
                            .text_size(px(17.0))
                            .font_weight(gpui::FontWeight::BOLD)
                            .child("MBX"),
                    )
                    .child(presets)
                    .child(self.chart_button(Command::Undo, cx))
                    .child(self.chart_button(Command::Redo, cx))
                    .child(div().flex_1())
                    .child(self.bands_button(cx))
                    .child(self.gain_button(&values, cx))
                    .children(self.fullscreen_button(cx)),
            )
            .child(
                div()
                    .flex()
                    .flex_1()
                    .min_h_0()
                    .children(self.bands_open.then(|| {
                        div()
                            .id("band-list")
                            .w(px(rail_width))
                            .min_h_0()
                            .overflow_y_scroll()
                            .flex_none()
                            .bg(rgb(RAIL))
                            .p(px(11.0))
                            .flex()
                            .flex_col()
                            .gap(px(6.0))
                            .child(
                                div()
                                    .flex()
                                    .justify_between()
                                    .px(px(9.0))
                                    .py(px(6.0))
                                    .text_color(rgb(RAIL_MUTED))
                                    .text_size(px(11.0))
                                    .child(div().child("Bands"))
                                    .child(div().child("Gain dB")),
                            )
                            .children(
                                layout
                                    .order()
                                    .iter()
                                    .copied()
                                    .map(|band| self.band_row(band, &values, cx)),
                            )
                            .children(can_add.then(|| self.chart_button(Command::Split, cx)))
                    }))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .px(px(18.0))
                            .pt(px(6.0))
                            .pb(px(5.0))
                            .flex()
                            .flex_col()
                            .gap(px(4.0))
                            .child(
                                div()
                                    .flex()
                                    .gap(px(8.0))
                                    .flex_1()
                                    .min_h_0()
                                    .child(
                                        div()
                                            .id("plot")
                                            .track_focus(&self.chart_focus)
                                            .tab_stop(true)
                                            .focus(|style| style.border_color(rgb(0x9a5550)))
                                            .on_key_down(cx.listener(Self::chart_key))
                                            .relative()
                                            .min_w_0()
                                            .flex_1()
                                            .min_h_0()
                                            .bg(rgb(PAPER))
                                            .rounded(px(5.0))
                                            .border_1()
                                            .border_color(rgb(HAIR))
                                            .on_mouse_down(
                                                MouseButton::Left,
                                                cx.listener(Self::chart_down),
                                            )
                                            .on_mouse_down(
                                                MouseButton::Right,
                                                cx.listener(Self::chart_solo),
                                            )
                                            .on_scroll_wheel(cx.listener(Self::chart_scroll))
                                            .child(spectrum)
                                            .children(threshold.map(|(_, marker, detector)| {
                                                marker.line(threshold_focused, detector)
                                            }))
                                            .child(
                                                div()
                                                    .absolute()
                                                    .top(px(plot::GRID_INSET))
                                                    .bottom(px(plot::FOOT + plot::GRID_INSET))
                                                    .right(px(5.0))
                                                    .w(px(plot::PAD - 9.0))
                                                    .text_right()
                                                    .text_size(px(9.0))
                                                    .text_color(rgb(MUTED))
                                                    .children(view.gain_ticks().map(
                                                        |(position, label)| {
                                                            div()
                                                                .absolute()
                                                                .top(relative(position))
                                                                .mt(px(-6.0))
                                                                .right_0()
                                                                .child(label)
                                                        },
                                                    )),
                                            )
                                            .children(graph_controls)
                                            .children(hover.map(|frequency| {
                                                div()
                                                    .absolute()
                                                    .top(px(7.0))
                                                    .left(px(plot::PAD))
                                                    .right(px(plot::PAD))
                                                    .h(px(20.0))
                                                    .child(
                                                        div()
                                                            .absolute()
                                                            .left(relative(
                                                                (view.position(frequency) as f32
                                                                    - 0.012)
                                                                    .clamp(0.0, 0.976),
                                                            ))
                                                            .size(px(20.0))
                                                            .rounded(px(4.0))
                                                            .bg(rgb(ACCENT))
                                                            .text_color(rgb(0x101010))
                                                            .flex()
                                                            .items_center()
                                                            .justify_center()
                                                            .text_size(px(14.0))
                                                            .child("+"),
                                                    )
                                            }))
                                            .child(
                                                div()
                                                    .absolute()
                                                    .bottom(px(plot::FOOT + 1.0))
                                                    .left(px(plot::PAD))
                                                    .right(px(plot::PAD))
                                                    .h(px(12.0))
                                                    .children(
                                                        plot::FREQUENCIES
                                                            .into_iter()
                                                            .zip(plot::LABELS)
                                                            .filter(|(frequency, _)| {
                                                                view.contains(plot::normalized(
                                                                    *frequency,
                                                                ))
                                                            })
                                                            .map(|(frequency, label)| {
                                                                let fraction = view.position(
                                                                    plot::normalized(frequency),
                                                                )
                                                                    as f32;
                                                                let start = (fraction - 0.015)
                                                                    .clamp(0.0, 0.968);
                                                                div()
                                                                    .absolute()
                                                                    .left(relative(start))
                                                                    .text_size(px(9.0))
                                                                    .text_color(rgb(MUTED))
                                                                    .child(label)
                                                            }),
                                                    ),
                                            )
                                            .children(detector.map(|_| {
                                                div()
                                                    .absolute()
                                                    .bottom(px(key::LANE_Y - 6.0))
                                                    .left(px(6.0))
                                                    .text_size(px(9.0))
                                                    .line_height(px(12.0))
                                                    .text_color(rgb(MUTED))
                                                    .child("KEY")
                                            }))
                                            .children(threshold_control)
                                            .children(threshold.map(|(_, marker, detector)| {
                                                marker.detector_dot(detector)
                                            }))
                                            .children(inspector),
                                    )
                                    .child(self.output_meter(cx)),
                            )
                            .child(
                                div()
                                    .pl(px(4.0))
                                    .text_size(px(11.0))
                                    .line_height(px(14.0))
                                    .text_color(rgb(MUTED))
                                    .child(guidance),
                            ),
                    ),
            )
            .child(
                div()
                    .h(px(FOOTER_HEIGHT))
                    .flex_none()
                    .bg(rgb(PAPER))
                    .px(px(12.0))
                    .border_t_1()
                    .border_color(rgb(HAIR))
                    .flex()
                    .items_center()
                    .gap(px(6.0))
                    .child(self.phase_selector(cx))
                    .child(div().w(px(144.0)).child(self.footer_value(LOOKAHEAD, cx)))
                    .child(div().w(px(81.0)).child(self.footer_value(BYPASS, cx)))
                    .child(div().w(px(86.0)).child(self.footer_value(MIX, cx)))
                    .child(div().w(px(110.0)).child(self.footer_value(OUTPUT, cx)))
                    .child(div().flex_1())
                    .child(self.trace_switch(0, "Pre", cx))
                    .child(self.trace_switch(1, "Post", cx))
                    .child(self.trace_switch(2, "SC", cx))
                    .child(self.trace_switch(TRACES, "Freeze", cx))
                    .children(self.size_button(cx)),
            )
            .children(preset_overlay)
    }
}

pub fn new(
    params: Arc<Parameters>,
    handler: Handler,
    meters: Arc<Meters>,
    comparison: Rc<RefCell<Comparison>>,
    presentation: Rc<Presentation>,
) -> toybox::gpui_gui::GpuiHostedGui {
    let edit_ids = Rc::new(RefCell::new(Vec::new()));
    let factory_ids = Rc::clone(&edit_ids);
    let cancel_handler = Rc::clone(&handler);
    let press = Rc::new(controls::PressSlot::default());
    let factory_press = Rc::clone(&press);
    let cancel_params = Arc::clone(&params);
    toybox::gpui_gui::GpuiHostedGui::new(
        "MbxEditorView",
        move |window, cx| {
            let params = Arc::clone(&params);
            let handler = Rc::clone(&handler);
            let meters = Arc::clone(&meters);
            let comparison = Rc::clone(&comparison);
            let presentation = Rc::clone(&presentation);
            let editing = Rc::clone(&factory_ids);
            let press = Rc::clone(&factory_press);
            cx.new(move |cx| {
                let _refresh = cx.spawn_in(window, async move |weak, cx| loop {
                    cx.background_executor()
                        .timer(Duration::from_millis(33))
                        .await;
                    if weak
                        .update_in(cx, |editor: &mut Editor, _, cx| {
                            let fall = if editor.freeze { 0.0 } else { 3.0 };
                            if !editor
                                .analyzer
                                .update(&mut editor.spectra, editor.traces, fall)
                                && fall > 0.0
                            {
                                for value in editor.spectra.iter_mut().flatten() {
                                    *value = (*value - fall).max(-100.0);
                                }
                            }
                            let peaks = std::array::from_fn(|channel| {
                                f32::from_bits(editor.meters.peak[channel].swap(0, Relaxed))
                            });
                            editor
                                .output
                                .update(editor.meters.output_channels.load(Relaxed), peaks);
                            for (db, peak) in editor.activity.iter_mut().zip(&editor.meters.gain) {
                                let value = f32::from_bits(peak.load(Relaxed));
                                *db = if value.signum() == db.signum() && value.abs() < db.abs() {
                                    db.signum() * (db.abs() - 0.7).max(value.abs())
                                } else {
                                    value
                                };
                            }
                            cx.notify();
                        })
                        .is_err()
                    {
                        break;
                    }
                });
                let snapshot = params.values();
                let browser = browser::Browser::new(comparison.borrow().active(), &snapshot);
                Editor {
                    params,
                    handler,
                    analyzer: Analyzer::new(Arc::clone(&meters)),
                    meters,
                    spectra: [[-100.0; POINTS]; TRACES],
                    traces: [true, true, false],
                    freeze: false,
                    output: output::Levels::default(),
                    peak_focus: cx.focus_handle(),
                    activity: [0.0; BANDS],
                    selected: 0,
                    selection: 0,
                    bands_focus: cx.focus_handle(),
                    phase_linear_focus: cx.focus_handle(),
                    bands_open: false,
                    presentation,
                    size_focus: std::array::from_fn(|_| cx.focus_handle()),
                    panel: None,
                    detector_open: false,
                    detector_focus: cx.focus_handle(),
                    hover: None,
                    hovered_band: None,
                    viewport: plot::Viewport::default(),
                    gain_scale: 0,
                    drag_db_pixels: 1.0,
                    drag_gain_db: 24.0,
                    gain_focus: cx.focus_handle(),
                    drag_initial: parameters::defaults(),
                    drag_edges: Vec::new(),
                    entry: None,
                    plot: Rc::new(Cell::new(None)),
                    chart_focus: cx.focus_handle(),
                    threshold_focus: cx.focus_handle(),
                    threshold_marker: None,
                    focus: std::array::from_fn(|_| cx.focus_handle()),
                    band_focus: std::array::from_fn(|_| cx.focus_handle()),
                    trace_focus: std::array::from_fn(|_| cx.focus_handle()),
                    command_focus: std::array::from_fn(|_| cx.focus_handle()),
                    comparison,
                    browser,
                    preset_focus: std::array::from_fn(|_| cx.focus_handle()),
                    graph_focus: std::array::from_fn(|_| cx.focus_handle()),
                    inspector_switch_focus: std::array::from_fn(|_| cx.focus_handle()),
                    mode_focus: std::array::from_fn(|_| std::array::from_fn(|_| cx.focus_handle())),
                    press,
                    journals: std::array::from_fn(|_| RefCell::new(journal::Journal::default())),
                    editing,
                    drag: None,
                    _refresh,
                }
            })
            .into()
        },
        view::DEFAULT.0,
        view::DEFAULT.1,
    )
    .with_size_contract(view::MINIMUM, view::DEFAULT, view::MAXIMUM)
    .with_pointer_cancel_callback(move || {
        controls::cancel_pointer(&press, &cancel_params, &cancel_handler, &edit_ids)
    })
}
