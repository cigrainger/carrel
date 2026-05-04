//! Leptos context provider and hooks for the keymap stack.

#[cfg(target_arch = "wasm32")]
use std::time::Duration;

use carrel_core::keymap::default::{self, BindingOverride};
#[cfg(target_arch = "wasm32")]
use carrel_core::keymap::{Action, DispatchResult, Key, Sequence};
use carrel_core::keymap::{KeymapStack, Layer, LayerId};
#[cfg(target_arch = "wasm32")]
use leptos::ev;
use leptos::prelude::*;
use leptos_router::NavigateOptions;
use leptos_router::hooks::use_navigate;

#[cfg(target_arch = "wasm32")]
use crate::api;
#[cfg(target_arch = "wasm32")]
use crate::keymap::event;
use crate::keymap::help::KeymapHelp;

type LocalSignal<T> = RwSignal<T, LocalStorage>;

#[derive(Clone)]
struct ActionHandler {
    id: u64,
    #[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
    action: String,
    #[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
    handler: UnsyncCallback<()>,
}

#[cfg(target_arch = "wasm32")]
#[derive(Clone)]
struct PendingSequence {
    keys: Vec<Key>,
    timer: Option<TimeoutHandle>,
}

/// Keymap state shared through Leptos context.
#[derive(Clone, Copy)]
pub(crate) struct KeymapContext {
    pub(crate) stack: LocalSignal<KeymapStack>,
    handlers: LocalSignal<Vec<ActionHandler>>,
    #[cfg(target_arch = "wasm32")]
    pending: LocalSignal<Option<PendingSequence>>,
    overrides: LocalSignal<Vec<BindingOverride>>,
    pub(crate) warnings: LocalSignal<Vec<String>>,
    pub(crate) help_open: LocalSignal<bool>,
    next_handler_id: LocalSignal<u64>,
}

impl KeymapContext {
    pub(crate) fn new() -> Self {
        Self {
            stack: RwSignal::new_local(KeymapStack::new()),
            handlers: RwSignal::new_local(Vec::new()),
            #[cfg(target_arch = "wasm32")]
            pending: RwSignal::new_local(None),
            overrides: RwSignal::new_local(Vec::new()),
            warnings: RwSignal::new_local(Vec::new()),
            help_open: RwSignal::new_local(false),
            next_handler_id: RwSignal::new_local(1),
        }
    }
}

/// Root keymap provider.
#[component]
pub fn KeymapProvider(children: Children) -> impl IntoView {
    let context = KeymapContext::new();
    provide_context(context);
    push_layer(context, default::global_layer());

    #[cfg(target_arch = "wasm32")]
    {
        use wasm_bindgen_futures::spawn_local;

        spawn_local(async move {
            match api::keymap_config().await {
                Ok(config) => apply_config(context, config),
                Err(error) => push_warning(
                    context,
                    format!("Could not load keymap config: {}", error.message()),
                ),
            }
        });

        let listener = window_event_listener(ev::keydown, move |event| {
            handle_keydown(context, event);
        });
        on_cleanup(move || listener.remove());
    }

    let children = children();

    view! {
        {children}
        <KeymapHelp/>
    }
}

/// Register global navigation actions.
#[component]
pub fn GlobalKeymapActions() -> impl IntoView {
    let navigate = use_navigate();

    let today = navigate.clone();
    use_action_handler("go-today", move || {
        today("/today", NavigateOptions::default());
    });

    let friends = navigate.clone();
    use_action_handler("go-friends", move || {
        friends("/friends", NavigateOptions::default());
    });

    let library = navigate.clone();
    use_action_handler("go-library", move || {
        library("/library", NavigateOptions::default());
    });

    let highlights = navigate.clone();
    use_action_handler("go-highlights", move || {
        highlights("/highlights", NavigateOptions::default());
    });

    use_action_handler("go-lists", move || {
        navigate("/lists", NavigateOptions::default());
    });
}

/// Push a keymap layer for the current component lifetime.
pub fn use_keymap_layer(layer: Layer) {
    let context = use_keymap_context();
    let id = push_layer(context, layer);

    if let Some(id) = id {
        on_cleanup(move || {
            context.stack.update(|stack| stack.pop(id));
        });
    }
}

/// Register an action handler for the current component lifetime.
pub fn use_action_handler(action: impl Into<String>, handler: impl Fn() + 'static) {
    let context = use_keymap_context();
    let id = context.next_handler_id.get_untracked();
    context.next_handler_id.set(id + 1);
    let action = action.into();
    let handler = UnsyncCallback::new(move |()| handler());

    context.handlers.update(|handlers| {
        handlers.push(ActionHandler {
            id,
            action,
            handler,
        });
    });

    on_cleanup(move || {
        context
            .handlers
            .update(|handlers| handlers.retain(|handler| handler.id != id));
    });
}

/// Return the keymap warning signal when the provider is mounted.
pub fn use_keymap_warnings() -> Option<LocalSignal<Vec<String>>> {
    use_context::<KeymapContext>().map(|context| context.warnings)
}

pub(crate) fn use_keymap_context() -> KeymapContext {
    use_context::<KeymapContext>().expect("KeymapProvider should be mounted")
}

pub(crate) fn push_layer(context: KeymapContext, mut layer: Layer) -> Option<LayerId> {
    let overrides = context.overrides.get_untracked();
    default::apply_overrides(&mut layer, &overrides);

    match context.stack.try_update(|stack| stack.push(layer)) {
        Some(Ok(id)) => Some(id),
        Some(Err(error)) => {
            push_warning(context, format!("Could not register keymap layer: {error}"));
            None
        }
        None => None,
    }
}

#[cfg(target_arch = "wasm32")]
fn apply_config(context: KeymapContext, config: api::KeymapConfig) {
    let mut warnings = config.warnings;
    let overrides = config
        .overrides
        .into_iter()
        .filter_map(|override_binding| {
            let sequence = match Sequence::parse(&override_binding.key) {
                Ok(sequence) => sequence,
                Err(error) => {
                    warnings.push(format!(
                        "Invalid keymap binding `{}`: {error}",
                        override_binding.key
                    ));
                    return None;
                }
            };

            Some(BindingOverride::new(
                override_binding.layer,
                sequence,
                Action::named(override_binding.action),
            ))
        })
        .collect::<Vec<_>>();

    context.warnings.set(warnings);
    context.overrides.set(overrides);
    reapply_overrides(context);
}

#[cfg(target_arch = "wasm32")]
fn reapply_overrides(context: KeymapContext) {
    let overrides = context.overrides.get_untracked();
    let mut warnings = Vec::new();

    context.stack.update(|stack| {
        for layer in stack.layers_mut() {
            let mut candidate = layer.clone();
            default::apply_overrides(&mut candidate, &overrides);

            let mut validation = KeymapStack::new();
            match validation.push(candidate.clone()) {
                Ok(_) => *layer = candidate,
                Err(error) => warnings.push(format!(
                    "Could not apply keymap overrides to {}: {error}",
                    layer.name()
                )),
            }
        }
    });

    if !warnings.is_empty() {
        context
            .warnings
            .update(|existing| existing.extend(warnings));
    }
}

fn push_warning(context: KeymapContext, warning: String) {
    context.warnings.update(|warnings| warnings.push(warning));
}

#[cfg(target_arch = "wasm32")]
fn handle_keydown(context: KeymapContext, event: web_sys::KeyboardEvent) {
    let Some(key) = event::key_from_event(&event) else {
        return;
    };

    if event::is_text_input_focused() && !event::is_global_passthrough_key(key) {
        return;
    }

    let mut keys = context
        .pending
        .get_untracked()
        .map(|pending| {
            if let Some(timer) = pending.timer {
                timer.clear();
            }
            pending.keys
        })
        .unwrap_or_default();
    keys.push(key);

    let Ok(sequence) = Sequence::new(keys.clone()) else {
        return;
    };

    match context
        .stack
        .with_untracked(|stack| stack.dispatch(&sequence))
    {
        DispatchResult::Action(action) => {
            context.pending.set(None);
            event.prevent_default();
            dispatch_action(context, action);
        }
        DispatchResult::PartialMatch => {
            event.prevent_default();
            let timer = set_timeout_with_handle(
                move || {
                    context.pending.set(None);
                },
                Duration::from_millis(500),
            )
            .ok();
            context.pending.set(Some(PendingSequence { keys, timer }));
        }
        DispatchResult::NoMatch => {
            context.pending.set(None);
        }
    }
}

#[cfg(target_arch = "wasm32")]
fn dispatch_action(context: KeymapContext, action: Action) {
    if context.help_open.get_untracked() && action.name() == "go-back" {
        context.help_open.set(false);
        return;
    }

    if action.name() == "show-keymap-help" {
        context.help_open.set(true);
        return;
    }

    let handler = context.handlers.with_untracked(|handlers| {
        handlers
            .iter()
            .rev()
            .find(|handler| handler.action == action.name())
            .cloned()
    });

    if let Some(handler) = handler {
        handler.handler.run(());
    }
}
