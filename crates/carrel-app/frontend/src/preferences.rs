//! Presentation preferences and passage-preserving layout changes.

use carrel_core::reading::{ListDensity, ReadingFont, ReadingPreferences};
use leptos::prelude::*;

#[derive(Clone, Copy, Default, PartialEq)]
pub(crate) enum SettingsSection {
    #[default]
    Reading,
    Sources,
    People,
    Devices,
    Keyboard,
}

#[derive(Clone, Copy)]
pub(crate) struct PreferencesController {
    pub(crate) current: RwSignal<ReadingPreferences>,
    pub(crate) appearance_open: RwSignal<bool>,
    pub(crate) settings_open: RwSignal<bool>,
    pub(crate) settings_section: RwSignal<SettingsSection>,
    #[cfg(target_arch = "wasm32")]
    anchor: StoredValue<Option<LayoutAnchor>, LocalStorage>,
}

impl PreferencesController {
    pub(crate) fn open_settings(self, section: SettingsSection) {
        self.appearance_open.set(false);
        self.settings_section.set(section);
        self.settings_open.set(true);
    }
    /// Retain the visible passage when shell geometry changes before the next paint.
    #[cfg(target_arch = "wasm32")]
    pub(crate) fn preserve_layout(self, change: impl FnOnce()) {
        let anchor = self
            .anchor
            .get_value()
            .filter(LayoutAnchor::reusable)
            .or_else(|| {
                web_sys::window()
                    .and_then(|window| window.document())
                    .and_then(|document| LayoutAnchor::capture(&document))
            });
        change();
        self.anchor.set_value(anchor.clone());
        if let Some(anchor) = anchor {
            use wasm_bindgen::{JsCast, closure::Closure};
            let expected_scroll = anchor.scroll.scroll_top();
            let callback = Closure::once_into_js(move || {
                // A route change or user scroll supersedes the pending restoration.
                if anchor.connected() && anchor.scroll.scroll_top() == expected_scroll {
                    anchor.restore();
                }
            });
            if let Some(window) = web_sys::window() {
                let _ = window.request_animation_frame(callback.unchecked_ref());
            }
        }
    }

    pub(crate) fn change(self, update: impl FnOnce(&mut ReadingPreferences)) {
        let mut next = self.current.get_untracked();
        update(&mut next);
        let next = next.normalized();
        if next == self.current.get_untracked() {
            return;
        }
        #[cfg(target_arch = "wasm32")]
        {
            let retained = apply(next, self.anchor.get_value());
            self.anchor.set_value(retained);
        }
        #[cfg(not(target_arch = "wasm32"))]
        apply(next);
        self.current.set(next);
        persist(next);
    }
}

pub(crate) fn provide_preferences() {
    let initial = load();
    #[cfg(target_arch = "wasm32")]
    apply(initial, None);
    #[cfg(not(target_arch = "wasm32"))]
    apply(initial);
    provide_context(PreferencesController {
        current: RwSignal::new(initial),
        appearance_open: RwSignal::new(false),
        settings_open: RwSignal::new(false),
        settings_section: RwSignal::new(SettingsSection::Reading),
        #[cfg(target_arch = "wasm32")]
        anchor: StoredValue::new_local(None),
    });
}

pub(crate) fn use_preferences() -> PreferencesController {
    expect_context()
}

fn font_value(font: ReadingFont) -> &'static str {
    match font {
        ReadingFont::SourceSerif => "source_serif",
        ReadingFont::Literata => "literata",
        ReadingFont::Garamond => "garamond",
        ReadingFont::Inter => "inter",
    }
}

#[component]
pub(crate) fn ReadingControls(#[prop(default = true)] density: bool) -> impl IntoView {
    let controller = use_preferences();
    let current = controller.current;
    view! {
        <fieldset class="reading-controls">
            <legend>"Typography"</legend>
            <label for="reading-font">"Font"</label>
            <select id="reading-font" prop:value=move || font_value(current.get().font) on:change=move |event| {
                let font = match event_target_value(&event).as_str() {
                    "literata" => ReadingFont::Literata,
                    "garamond" => ReadingFont::Garamond,
                    "inter" => ReadingFont::Inter,
                    _ => ReadingFont::SourceSerif,
                };
                controller.change(|p| p.font = font);
            }>
                <option value="source_serif">"Source Serif"</option><option value="literata">"Literata"</option>
                <option value="garamond">"EB Garamond"</option><option value="inter">"Inter"</option>
            </select>
            <label for="reading-size">"Text size" <output>{move || format!("{}px", current.get().size)}</output></label>
            <input id="reading-size" type="range" min="14" max="26" step="1" prop:value=move || current.get().size on:input=move |event| {
                if let Ok(value) = event_target_value(&event).parse() { controller.change(|p| p.size = value); }
            }/>
            <label for="reading-width">"Line width" <output>{move || format!("{}ch", current.get().measure)}</output></label>
            <input id="reading-width" type="range" min="52" max="82" step="2" prop:value=move || current.get().measure on:input=move |event| {
                if let Ok(value) = event_target_value(&event).parse() { controller.change(|p| p.measure = value); }
            }/>
            <label for="reading-spacing">"Line spacing" <output>{move || format!("{:.2}", f64::from(current.get().leading) / 100.0)}</output></label>
            <input id="reading-spacing" type="range" min="145" max="185" step="5" prop:value=move || current.get().leading on:input=move |event| {
                if let Ok(value) = event_target_value(&event).parse() { controller.change(|p| p.leading = value); }
            }/>
            <button type="button" class="preference-reset" on:click=move |_| controller.change(|p| *p = p.reset_reading())>"Reset reading"</button>
        </fieldset>
        <Show when=move || density><fieldset class="density-controls">
            <legend>"Article lists"</legend>
            <div class="density-options" role="group" aria-label="List density">
                <button type="button" aria-pressed=move || (current.get().density == ListDensity::Editorial).to_string() on:click=move |_| controller.change(|p| p.density = ListDensity::Editorial)>"Editorial"</button>
                <button type="button" aria-pressed=move || (current.get().density == ListDensity::Dense).to_string() on:click=move |_| controller.change(|p| p.density = ListDensity::Dense)>"Dense"</button>
            </div>
        </fieldset></Show>
    }
}

#[cfg(target_arch = "wasm32")]
const STORAGE_KEY: &str = "carrel.reading";

#[cfg(target_arch = "wasm32")]
fn load() -> ReadingPreferences {
    web_sys::window()
        .and_then(|window| window.local_storage().ok().flatten())
        .and_then(|storage| storage.get_item(STORAGE_KEY).ok().flatten())
        .and_then(|value| serde_json::from_str::<ReadingPreferences>(&value).ok())
        .unwrap_or_default()
        .normalized()
}

#[cfg(not(target_arch = "wasm32"))]
fn load() -> ReadingPreferences {
    ReadingPreferences::default()
}

#[cfg(target_arch = "wasm32")]
fn persist(preferences: ReadingPreferences) {
    let saved = web_sys::window()
        .and_then(|window| window.local_storage().ok().flatten())
        .zip(serde_json::to_string(&preferences).ok())
        .is_some_and(|(storage, value)| storage.set_item(STORAGE_KEY, &value).is_ok());
    if !saved {
        crate::status::use_status_controller()
            .warn("Appearance changed, but could not be remembered on this installation.");
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn persist(_preferences: ReadingPreferences) {}

#[cfg(target_arch = "wasm32")]
fn apply(preferences: ReadingPreferences, previous: Option<LayoutAnchor>) -> Option<LayoutAnchor> {
    let document = web_sys::window().and_then(|window| window.document())?;
    let root = document.document_element()?;
    let anchor = previous
        .filter(LayoutAnchor::reusable)
        .or_else(|| LayoutAnchor::capture(&document));
    let font = match preferences.font {
        ReadingFont::SourceSerif => "\"Source Serif 4\", Georgia, serif",
        ReadingFont::Literata => "Literata, Georgia, serif",
        ReadingFont::Garamond => "\"EB Garamond\", Georgia, serif",
        ReadingFont::Inter => "Inter, sans-serif",
    };
    let size = preferences.size + u8::from(preferences.font == ReadingFont::Garamond);
    let style = format!(
        "--font-body:{font};--reading-size:{size}px;--reading-measure:{}ch;--reading-line-height:{};",
        preferences.measure,
        f64::from(preferences.leading) / 100.0
    );
    let _ = root.set_attribute("style", &style);
    let _ = root.set_attribute(
        "data-density",
        if preferences.density == ListDensity::Dense {
            "dense"
        } else {
            "editorial"
        },
    );
    if let Some(anchor) = anchor.clone() {
        anchor.restore();
        let expected_scroll = anchor.scroll.scroll_top();
        // A locally bundled face can still load after the first reflow. Retain
        // the passage only if no newer preference or user scroll supersedes it.
        let loading = document.fonts().load(&format!("{size}px {font}"));
        wasm_bindgen_futures::spawn_local(async move {
            let _ = wasm_bindgen_futures::JsFuture::from(loading).await;
            if anchor.scroll.is_connected()
                && anchor.scroll.scroll_top() == expected_scroll
                && root.get_attribute("style").as_deref() == Some(&style)
            {
                anchor.restore();
            }
        });
    }
    anchor
}

#[cfg(not(target_arch = "wasm32"))]
fn apply(_preferences: ReadingPreferences) {}

/// A text character or list row stays at the same viewport position after reflow.
#[cfg(target_arch = "wasm32")]
#[derive(Clone)]
struct LayoutAnchor {
    scroll: web_sys::Element,
    target: AnchorTarget,
    top: f64,
    last_scroll: std::rc::Rc<std::cell::Cell<i32>>,
}

#[cfg(target_arch = "wasm32")]
#[derive(Clone)]
enum AnchorTarget {
    Text(web_sys::Range),
    Row(web_sys::Element),
}

#[cfg(target_arch = "wasm32")]
impl LayoutAnchor {
    fn capture(document: &web_sys::Document) -> Option<Self> {
        if let Some(scroll) = document.query_selector(".reader-scroll").ok().flatten() {
            if scroll.scroll_top() == 0 {
                return None;
            }
            let body = document.query_selector(".reader-body").ok().flatten()?;
            let line = scroll.get_bounding_client_rect().top() + 2.0;
            let mut nodes = vec![web_sys::Node::from(body)];
            while let Some(node) = nodes.pop() {
                if node.node_type() == web_sys::Node::TEXT_NODE {
                    let length = node
                        .text_content()
                        .unwrap_or_default()
                        .encode_utf16()
                        .count() as u32;
                    if length == 0 {
                        continue;
                    }
                    let range = document.create_range().ok()?;
                    range.select_node_contents(&node).ok()?;
                    let rect = range.get_bounding_client_rect();
                    if rect.bottom() < line || rect.height() == 0.0 {
                        continue;
                    }
                    let (mut low, mut high) = (0, length - 1);
                    while low < high {
                        let mid = low + (high - low) / 2;
                        range.set_start(&node, mid).ok()?;
                        range.set_end(&node, mid + 1).ok()?;
                        if range.get_bounding_client_rect().bottom() < line {
                            low = mid + 1;
                        } else {
                            high = mid;
                        }
                    }
                    range.set_start(&node, low).ok()?;
                    range.set_end(&node, low + 1).ok()?;
                    let top = range.get_bounding_client_rect().top();
                    return Some(Self {
                        last_scroll: std::rc::Rc::new(std::cell::Cell::new(scroll.scroll_top())),
                        scroll,
                        target: AnchorTarget::Text(range),
                        top,
                    });
                }
                let children = node.child_nodes();
                for index in (0..children.length()).rev() {
                    if let Some(child) = children.item(index) {
                        nodes.push(child);
                    }
                }
            }
        } else {
            let scroll = document.get_element_by_id("main-content")?;
            if scroll.scroll_top() == 0 {
                return None;
            }
            let line = scroll.get_bounding_client_rect().top();
            let rows = scroll.query_selector_all(".item-row").ok()?;
            use wasm_bindgen::JsCast;
            for index in 0..rows.length() {
                let row = rows.item(index)?.dyn_into::<web_sys::Element>().ok()?;
                let rect = row.get_bounding_client_rect();
                if rect.bottom() >= line {
                    return Some(Self {
                        last_scroll: std::rc::Rc::new(std::cell::Cell::new(scroll.scroll_top())),
                        scroll,
                        target: AnchorTarget::Row(row),
                        top: rect.top(),
                    });
                }
            }
        }
        None
    }

    fn current_top(&self) -> f64 {
        match &self.target {
            AnchorTarget::Text(range) => range.get_bounding_client_rect().top(),
            AnchorTarget::Row(row) => row.get_bounding_client_rect().top(),
        }
    }

    fn connected(&self) -> bool {
        self.scroll.is_connected()
            && match &self.target {
                AnchorTarget::Text(range) => range
                    .start_container()
                    .is_ok_and(|node| node.is_connected()),
                AnchorTarget::Row(row) => row.is_connected(),
            }
    }

    fn reusable(&self) -> bool {
        self.connected()
            && self.scroll.scroll_top() == self.last_scroll.get()
            && (self.current_top() - self.top).abs() <= 2.0
    }

    fn restore(&self) {
        let top = self.current_top();
        self.scroll
            .set_scroll_top(self.scroll.scroll_top() + (top - self.top).round() as i32);
        self.last_scroll.set(self.scroll.scroll_top());
    }
}
