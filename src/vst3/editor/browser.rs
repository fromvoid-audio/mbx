use super::{
    mark_dirty, Editor, ACCENT, HAIR, HEADER_GAP, HEADER_HEIGHT, HEADER_PADDING, MUTED, PAPER,
    TEXT, TITLE_WIDTH,
};
use crate::{
    parameters::{layout::Layout, Values},
    vst3::{
        comparison::Slot,
        presets::{self, Source, FACTORY},
    },
};
use std::{io, path::PathBuf};
use toybox::gpui::{
    self, div, px, rgb, AnyElement, Context, InteractiveElement, IntoElement, MouseButton,
    ParentElement, ScrollHandle, SharedString, StatefulInteractiveElement, Styled, Window,
};

#[derive(Clone)]
struct Reference {
    source: Source,
    values: Values,
}

pub struct Browser {
    folder: Option<PathBuf>,
    items: Vec<Source>,
    factory: Vec<Values>,
    reference: [Option<Reference>; 2],
    pub open: bool,
    cursor: usize,
    scroll: ScrollHandle,
    draft: Option<NameDraft>,
    message: Option<String>,
}

#[derive(Default)]
struct NameDraft {
    text: Vec<char>,
    caret: usize,
    selected: bool,
}

impl NameDraft {
    fn replace_selection(&mut self) {
        if self.selected {
            self.text.clear();
            self.caret = 0;
            self.selected = false;
        }
    }

    fn insert(&mut self, text: &str) {
        self.replace_selection();
        let inserted: Vec<_> = text
            .chars()
            .filter(|ch| !ch.is_control())
            .take(60 - self.text.len())
            .collect();
        let count = inserted.len();
        self.text.splice(self.caret..self.caret, inserted);
        self.caret += count;
    }

    fn display(&self) -> String {
        if self.text.is_empty() {
            return "Preset name".to_owned();
        }
        let mut text = self.text.clone();
        text.insert(self.caret, '▏');
        text.into_iter().collect()
    }
}

impl Browser {
    pub fn new(slot: Slot, current: &Values) -> Self {
        let mut browser = Self {
            folder: presets::default_folder(),
            items: (0..FACTORY.len()).map(Source::Factory).collect(),
            factory: FACTORY.iter().map(|preset| preset.values()).collect(),
            reference: [None, None],
            open: false,
            cursor: 0,
            scroll: ScrollHandle::new(),
            draft: None,
            message: None,
        };
        if let Some(index) = browser.factory.iter().position(|values| values == current) {
            browser.reference[slot.index()] = Some(Reference {
                source: Source::Factory(index),
                values: *current,
            });
        }
        browser
    }

    fn refresh(&mut self) {
        match presets::catalog(self.folder.as_deref()) {
            Ok(items) => {
                self.items = items;
                self.cursor = self.cursor.min(self.items.len() - 1);
                self.message = None;
            }
            Err(error) => self.message = Some(format!("Could not read user presets: {error}")),
        }
    }

    fn reference(&self, slot: Slot, values: &Values) -> Option<(Source, bool)> {
        let current = self.reference[slot.index()].as_ref();
        if let Some(current) = current.filter(|current| current.values == *values) {
            return Some((current.source.clone(), false));
        }
        if let Some(index) = self.factory.iter().position(|factory| factory == values) {
            return Some((Source::Factory(index), false));
        }
        current.map(|reference| (reference.source.clone(), true))
    }

    fn current_index(&self, slot: Slot, values: &Values) -> Option<usize> {
        let (source, _) = self.reference(slot, values)?;
        self.items.iter().position(|item| *item == source)
    }
}

#[repr(usize)]
#[derive(Clone, Copy)]
enum Action {
    Previous,
    Menu,
    Next,
    A,
    B,
    Copy,
    Popup,
}

impl Editor {
    fn choose_preset(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        let source = self.browser.items[index].clone();
        match source.values() {
            Ok(values) => {
                let slot = self.slot().index();
                self.apply_values(values);
                self.browser.reference[slot] = Some(Reference { source, values });
                let layout = Layout::new(&values);
                if !layout.order().contains(&self.selected) {
                    if let Some(band) = layout.at(0.5).or_else(|| layout.order().first().copied()) {
                        self.selected = band;
                        self.selection = super::one(band);
                    }
                }
                if self.browser.open {
                    self.close_presets(window, cx);
                } else {
                    cx.notify();
                }
            }
            Err(error) => {
                self.browser.open = true;
                self.browser.cursor = index;
                self.browser.message = Some(format!("Could not open {}: {error}", source.name()));
                window.focus(&self.preset_focus[Action::Popup as usize], cx);
                cx.notify();
            }
        }
    }

    fn close_presets(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.browser.open = false;
        self.browser.draft = None;
        self.browser.message = None;
        window.focus(&self.preset_focus[Action::Menu as usize], cx);
        cx.notify();
    }

    fn preset_action(&mut self, action: Action, window: &mut Window, cx: &mut Context<Self>) {
        match action {
            Action::Menu => {
                if self.browser.open {
                    self.close_presets(window, cx);
                } else {
                    self.finish_edit();
                    self.entry = None;
                    self.browser.refresh();
                    self.browser.cursor = self
                        .browser
                        .current_index(self.slot(), &self.params.values())
                        .unwrap_or(0);
                    self.browser.open = true;
                    self.browser.scroll.scroll_to_item(self.browser.cursor);
                    window.focus(&self.preset_focus[Action::Popup as usize], cx);
                }
            }
            Action::Previous | Action::Next => {
                self.browser.refresh();
                let step = if matches!(action, Action::Previous) {
                    -1
                } else {
                    1
                };
                let count = self.browser.items.len() as isize;
                let start = self
                    .browser
                    .current_index(self.slot(), &self.params.values())
                    .map_or_else(|| if step > 0 { -1 } else { 0 }, |index| index as isize);
                self.choose_preset((start + step).rem_euclid(count) as usize, window, cx);
            }
            Action::A => self.switch_to(Slot::A, cx),
            Action::B => self.switch_to(Slot::B, cx),
            Action::Copy => {
                let slot = self.slot();
                let values = self.params.values();
                self.comparison.borrow_mut().copy(values);
                self.browser.reference[slot.other().index()] =
                    self.browser.reference[slot.index()].clone();
                self.journals[slot.other().index()].take();
                mark_dirty(&self.handler);
            }
            Action::Popup => (),
        }
        cx.notify();
    }

    fn save_preset(&mut self, cx: &mut Context<Self>) {
        let Some(draft) = &self.browser.draft else {
            return;
        };
        let name: String = draft.text.iter().collect();
        let values = self.params.values();
        let Some(folder) = self.browser.folder.as_ref() else {
            self.browser.message = Some("User preset folder is unavailable".to_owned());
            cx.notify();
            return;
        };
        match presets::save_file(folder, &name, &values) {
            Ok(source) => {
                let slot = self.slot().index();
                self.browser.reference[slot] = Some(Reference { source, values });
                self.browser.draft = None;
                self.browser.refresh();
                self.browser.cursor = self
                    .browser
                    .current_index(self.slot(), &values)
                    .unwrap_or(0);
            }
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
                self.browser.message = Some("A preset with that name already exists".to_owned());
            }
            Err(error) => self.browser.message = Some(format!("{error}")),
        }
        cx.notify();
    }

    fn preset_key(
        &mut self,
        event: &gpui::KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let stroke = &event.keystroke;
        if let Some(draft) = self.browser.draft.as_mut() {
            match stroke.key.as_str() {
                "escape" => {
                    self.browser.draft = None;
                    self.browser.message = None;
                }
                "enter" => self.save_preset(cx),
                "a" if stroke.modifiers.platform => draft.selected = true,
                "v" if stroke.modifiers.platform => {
                    if let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) {
                        draft.insert(&text);
                    }
                }
                "left" => {
                    draft.caret = draft.caret.saturating_sub(1);
                    draft.selected = false;
                }
                "right" => {
                    draft.caret = (draft.caret + 1).min(draft.text.len());
                    draft.selected = false;
                }
                "home" => {
                    draft.caret = 0;
                    draft.selected = false;
                }
                "end" => {
                    draft.caret = draft.text.len();
                    draft.selected = false;
                }
                "backspace" if draft.selected => draft.replace_selection(),
                "backspace" if draft.caret > 0 => {
                    draft.caret -= 1;
                    draft.text.remove(draft.caret);
                }
                "delete" if draft.selected => draft.replace_selection(),
                "delete" if draft.caret < draft.text.len() => {
                    draft.text.remove(draft.caret);
                }
                _ if !stroke.modifiers.platform && !stroke.modifiers.control => {
                    if let Some(text) = stroke.key_char.as_deref() {
                        draft.insert(text);
                    }
                }
                _ => (),
            }
        } else {
            match stroke.key.as_str() {
                "escape" => self.close_presets(window, cx),
                "up" => self.browser.cursor = self.browser.cursor.saturating_sub(1),
                "down" => {
                    self.browser.cursor =
                        (self.browser.cursor + 1).min(self.browser.items.len() - 1)
                }
                "home" => self.browser.cursor = 0,
                "end" => self.browser.cursor = self.browser.items.len() - 1,
                "enter" => self.choose_preset(self.browser.cursor, window, cx),
                "s" if stroke.modifiers.platform => self.browser.draft = Some(NameDraft::default()),
                _ => (),
            }
        }
        self.browser.scroll.scroll_to_item(self.browser.cursor);
        cx.stop_propagation();
        cx.notify();
    }

    fn preset_button(
        &self,
        action: Action,
        label: impl Into<SharedString>,
        width: f32,
        style: ButtonStyle,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let focus = self.preset_focus[action as usize].clone();
        div()
            .id(("preset-action", action as usize))
            .track_focus(&focus)
            .tab_stop(style.enabled)
            .w(px(width))
            .min_w_0()
            .h(px(26.0))
            .flex()
            .items_center()
            .justify_center()
            .text_size(px(11.0))
            .text_color(rgb(if style.active {
                TEXT
            } else if style.dimmed || !style.enabled {
                MUTED
            } else {
                TEXT
            }))
            .border_1()
            .rounded(px(4.0))
            .border_color(rgb(if style.active { ACCENT } else { HAIR }))
            .bg(rgb(if style.active { 0x442321 } else { PAPER }))
            .focus(|style| style.border_color(rgb(ACCENT)))
            .hover(|style| style.bg(rgb(0x373737)))
            .cursor_pointer()
            .child(div().truncate().px(px(5.0)).child(label.into()))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, _, window, cx| {
                    if style.enabled {
                        window.focus(&focus, cx);
                        this.preset_action(action, window, cx);
                    }
                }),
            )
            .on_key_down(
                cx.listener(move |this, event: &gpui::KeyDownEvent, window, cx| {
                    if style.enabled
                        && matches!(event.keystroke.key.as_str(), "space" | "enter" | "down")
                    {
                        this.preset_action(action, window, cx);
                        cx.stop_propagation();
                    }
                }),
            )
            .into_any_element()
    }

    pub fn preset_strip(&self, values: &Values, cx: &mut Context<Self>) -> AnyElement {
        let active = self.slot();
        let reference = self.browser.reference(active, values);
        let name = reference
            .as_ref()
            .map_or("Custom", |(source, _)| source.name());
        let edited = reference.as_ref().is_some_and(|(_, edited)| *edited);
        let title = format!("{name}{}  ▾", if edited { " *" } else { "" });
        let comparison = self.comparison.borrow();
        div()
            .flex()
            .items_center()
            .gap(px(4.0))
            .child(self.preset_button(Action::Previous, "‹", 26.0, ButtonStyle::default(), cx))
            .child(self.preset_button(
                Action::Menu,
                title,
                158.0,
                ButtonStyle {
                    dimmed: edited,
                    ..ButtonStyle::default()
                },
                cx,
            ))
            .child(self.preset_button(Action::Next, "›", 26.0, ButtonStyle::default(), cx))
            .child(div().w(px(1.0)).h(px(14.0)).mx(px(3.0)).bg(rgb(HAIR)))
            .child(self.preset_button(
                Action::A,
                "A",
                28.0,
                ButtonStyle {
                    active: active == Slot::A,
                    ..ButtonStyle::default()
                },
                cx,
            ))
            .child(self.preset_button(
                Action::B,
                "B",
                28.0,
                ButtonStyle {
                    active: active == Slot::B,
                    ..ButtonStyle::default()
                },
                cx,
            ))
            .child(self.preset_button(
                Action::Copy,
                "Copy",
                48.0,
                ButtonStyle {
                    enabled: comparison.differs(values),
                    ..ButtonStyle::default()
                },
                cx,
            ))
            .into_any_element()
    }

    pub fn preset_overlay(&self, values: &Values, cx: &mut Context<Self>) -> AnyElement {
        let focus = self.preset_focus[Action::Popup as usize].clone();
        let current = self.browser.current_index(self.slot(), values);
        let cursor = self.browser.cursor;
        let draft = self.browser.draft.as_ref();
        let rows = self
            .browser
            .items
            .iter()
            .enumerate()
            .map(|(index, source)| {
                let first_user = index == FACTORY.len();
                let description = source.description();
                let row = div()
                    .id(("preset-row", index))
                    .px(px(10.0))
                    .py(px(5.0))
                    .bg(rgb(if cursor == index { 0x383838 } else { PAPER }))
                    .hover(|style| style.bg(rgb(0x414141)))
                    .cursor_pointer()
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap(px(9.0))
                    .child(
                        div()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .child(div().truncate().child(source.name().to_owned()))
                            .children((!description.is_empty()).then(|| {
                                div()
                                    .text_size(px(10.0))
                                    .text_color(rgb(MUTED))
                                    .child(description)
                            })),
                    )
                    .children(
                        (current == Some(index)).then(|| div().text_color(rgb(ACCENT)).child("✓")),
                    )
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, _, window, cx| {
                            this.choose_preset(index, window, cx)
                        }),
                    );
                div()
                    .children(first_user.then(|| section("User")))
                    .child(row)
            });
        let panel = div()
            .id("presets-popup")
            .track_focus(&focus)
            .absolute()
            .left(px(HEADER_PADDING + TITLE_WIDTH + HEADER_GAP))
            .top(px(HEADER_HEIGHT - 2.0))
            .w(px(332.0))
            .p(px(6.0))
            .flex()
            .flex_col()
            .gap(px(4.0))
            .bg(rgb(PAPER))
            .text_color(rgb(TEXT))
            .border_1()
            .border_color(rgb(HAIR))
            .rounded(px(6.0))
            .text_size(px(12.0))
            .occlude()
            .on_key_down(cx.listener(Self::preset_key))
            .child(section("Factory"))
            .child(
                div()
                    .id("preset-scroll")
                    .max_h(px(330.0))
                    .overflow_y_scroll()
                    .track_scroll(&self.browser.scroll)
                    .children(rows),
            )
            .children(draft.map(|draft| {
                div()
                    .px(px(8.0))
                    .py(px(6.0))
                    .rounded(px(3.0))
                    .border_1()
                    .border_color(rgb(ACCENT))
                    .child(draft.display())
            }))
            .children(self.browser.message.as_ref().map(|message| {
                div()
                    .px(px(6.0))
                    .text_size(px(11.0))
                    .text_color(rgb(0xff8379))
                    .child(message.clone())
            }))
            .child(
                div()
                    .border_t_1()
                    .border_color(rgb(HAIR))
                    .pt(px(5.0))
                    .flex()
                    .justify_between()
                    .child(
                        div()
                            .id("save-user-preset")
                            .cursor_pointer()
                            .px(px(7.0))
                            .py(px(5.0))
                            .hover(|style| style.bg(rgb(0x393939)))
                            .child(if draft.is_some() {
                                "Save"
                            } else {
                                "Save as…"
                            })
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(|this, _, window, cx| {
                                    if this.browser.draft.is_some() {
                                        this.save_preset(cx);
                                    } else {
                                        this.browser.draft = Some(NameDraft::default());
                                        this.browser.message = None;
                                    }
                                    window.focus(&this.preset_focus[Action::Popup as usize], cx);
                                    cx.notify();
                                }),
                            ),
                    )
                    .child(
                        div()
                            .id("close-presets")
                            .cursor_pointer()
                            .px(px(7.0))
                            .py(px(5.0))
                            .hover(|style| style.bg(rgb(0x393939)))
                            .child("Close")
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(|this, _, window, cx| this.close_presets(window, cx)),
                            ),
                    ),
            );
        div()
            .absolute()
            .size_full()
            .child(
                div()
                    .id("presets-background")
                    .absolute()
                    .size_full()
                    .occlude()
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|this, _, window, cx| this.close_presets(window, cx)),
                    ),
            )
            .child(panel)
            .into_any_element()
    }
}

#[derive(Clone, Copy)]
struct ButtonStyle {
    active: bool,
    dimmed: bool,
    enabled: bool,
}

impl Default for ButtonStyle {
    fn default() -> Self {
        Self {
            active: false,
            dimmed: false,
            enabled: true,
        }
    }
}

fn section(label: &'static str) -> impl IntoElement {
    div()
        .px(px(8.0))
        .py(px(3.0))
        .text_size(px(10.0))
        .text_color(rgb(MUTED))
        .child(label)
}
