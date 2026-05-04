//! Stack-based keymap dispatch.

use crate::keymap::{Action, Binding, KeymapError, Sequence};

/// Stable identifier assigned when a layer is pushed.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct LayerId(u64);

impl LayerId {
    const UNASSIGNED: Self = Self(0);

    /// Return the raw numeric id.
    pub const fn get(self) -> u64 {
        self.0
    }
}

/// A named set of key bindings.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Layer {
    id: LayerId,
    key: String,
    name: String,
    bindings: Vec<Binding>,
}

impl Layer {
    /// Create a layer. A real id is assigned when pushed onto a stack.
    pub fn new(key: impl Into<String>, name: impl Into<String>, bindings: Vec<Binding>) -> Self {
        Self {
            id: LayerId::UNASSIGNED,
            key: key.into(),
            name: name.into(),
            bindings,
        }
    }

    /// Stable key used for configuration overrides.
    pub fn key(&self) -> &str {
        &self.key
    }

    /// Human-readable layer name.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Assigned layer id.
    pub const fn id(&self) -> LayerId {
        self.id
    }

    /// Bindings in this layer.
    pub fn bindings(&self) -> &[Binding] {
        &self.bindings
    }

    /// Mutable bindings in this layer.
    pub fn bindings_mut(&mut self) -> &mut Vec<Binding> {
        &mut self.bindings
    }
}

/// Result of dispatching a key sequence.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DispatchResult {
    /// A binding matched exactly.
    Action(Action),
    /// A binding starts with this sequence; wait for another key.
    PartialMatch,
    /// No binding matched.
    NoMatch,
}

/// Stack of active keymap layers.
#[derive(Clone, Debug)]
pub struct KeymapStack {
    layers: Vec<Layer>,
    next_id: u64,
}

impl Default for KeymapStack {
    fn default() -> Self {
        Self::new()
    }
}

impl KeymapStack {
    /// Create an empty keymap stack.
    pub const fn new() -> Self {
        Self {
            layers: Vec::new(),
            next_id: 1,
        }
    }

    /// Push a layer and return its assigned id.
    pub fn push(&mut self, mut layer: Layer) -> Result<LayerId, KeymapError> {
        validate_layer(&layer)?;

        let id = LayerId(self.next_id);
        self.next_id += 1;
        layer.id = id;
        self.layers.push(layer);
        Ok(id)
    }

    /// Remove a layer by id. Removing from the middle is allowed.
    pub fn pop(&mut self, id: LayerId) {
        if let Some(index) = self.layers.iter().position(|layer| layer.id == id) {
            self.layers.remove(index);
        }
    }

    /// Return active layers from bottom to top.
    pub fn layers(&self) -> &[Layer] {
        &self.layers
    }

    /// Return mutable active layers from bottom to top.
    pub fn layers_mut(&mut self) -> &mut [Layer] {
        &mut self.layers
    }

    /// Dispatch a key sequence against the active stack.
    pub fn dispatch(&self, sequence: &Sequence) -> DispatchResult {
        for layer in self.layers.iter().rev() {
            if let Some(binding) = layer
                .bindings
                .iter()
                .find(|binding| binding.sequence == *sequence)
            {
                return DispatchResult::Action(binding.action.clone());
            }

            if layer
                .bindings
                .iter()
                .any(|binding| sequence.is_prefix_of(&binding.sequence))
            {
                return DispatchResult::PartialMatch;
            }
        }

        DispatchResult::NoMatch
    }

    /// Return active bindings from top layer to bottom layer.
    pub fn current_bindings(&self) -> impl Iterator<Item = &Binding> {
        self.layers.iter().rev().flat_map(Layer::bindings)
    }
}

fn validate_layer(layer: &Layer) -> Result<(), KeymapError> {
    for (index, first) in layer.bindings.iter().enumerate() {
        for second in layer.bindings.iter().skip(index + 1) {
            if first.sequence.conflicts_with(&second.sequence) {
                return Err(KeymapError::ConflictingBinding {
                    layer: layer.key.clone(),
                    first: first.sequence.to_string(),
                    second: second.sequence.to_string(),
                });
            }
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use crate::keymap::{Key, KeyCode, Modifiers};

    use super::*;

    fn binding(key: char, action: &str) -> Binding {
        Binding::new(
            Sequence::single(Key::character(key)),
            Action::named(action),
            action,
        )
    }

    fn layer(key: &str, bindings: Vec<Binding>) -> Layer {
        Layer::new(key, key, bindings)
    }

    #[test]
    fn push_and_pop_change_the_top_layer() {
        let mut stack = KeymapStack::new();
        let global = stack
            .push(layer("global", vec![binding('a', "global")]))
            .unwrap();
        let route = stack
            .push(layer("route", vec![binding('b', "route")]))
            .unwrap();

        assert_eq!(stack.layers().last().unwrap().id(), route);
        stack.pop(route);
        assert_eq!(stack.layers().last().unwrap().id(), global);
    }

    #[test]
    fn pop_removes_non_top_layer_from_the_middle() {
        let mut stack = KeymapStack::new();
        let global = stack
            .push(layer("global", vec![binding('a', "global")]))
            .unwrap();
        let middle = stack
            .push(layer("middle", vec![binding('b', "middle")]))
            .unwrap();
        let top = stack.push(layer("top", vec![binding('c', "top")])).unwrap();

        stack.pop(middle);

        let ids = stack.layers().iter().map(Layer::id).collect::<Vec<_>>();
        assert_eq!(ids, vec![global, top]);
    }

    #[test]
    fn dispatch_uses_the_top_layer_first() {
        let mut stack = KeymapStack::new();
        stack
            .push(layer("global", vec![binding('j', "global-next")]))
            .unwrap();
        stack
            .push(layer("route", vec![binding('j', "route-next")]))
            .unwrap();

        let result = stack.dispatch(&Sequence::single(Key::character('j')));

        assert_eq!(result, DispatchResult::Action(Action::named("route-next")));
    }

    #[test]
    fn dispatch_falls_through_to_lower_layers() {
        let mut stack = KeymapStack::new();
        stack
            .push(layer("global", vec![binding('?', "help")]))
            .unwrap();
        stack
            .push(layer("route", vec![binding('j', "next")]))
            .unwrap();

        let result = stack.dispatch(&Sequence::single(Key::character('?')));

        assert_eq!(result, DispatchResult::Action(Action::named("help")));
    }

    #[test]
    fn dispatch_reports_partial_matches() {
        let mut stack = KeymapStack::new();
        stack
            .push(layer(
                "list",
                vec![Binding::new(
                    Sequence::parse("g g").unwrap(),
                    Action::named("top"),
                    "top",
                )],
            ))
            .unwrap();

        let result = stack.dispatch(&Sequence::single(Key::character('g')));

        assert_eq!(result, DispatchResult::PartialMatch);
    }

    #[test]
    fn dispatch_reports_no_match() {
        let mut stack = KeymapStack::new();
        stack
            .push(layer("list", vec![binding('j', "next")]))
            .unwrap();

        let result = stack.dispatch(&Sequence::single(Key::character('x')));

        assert_eq!(result, DispatchResult::NoMatch);
    }

    #[test]
    fn layer_registration_rejects_prefix_conflicts() {
        let mut stack = KeymapStack::new();
        let result = stack.push(layer(
            "bad",
            vec![
                binding('g', "one"),
                Binding::new(Sequence::parse("g g").unwrap(), Action::named("two"), "two"),
            ],
        ));

        assert!(matches!(
            result,
            Err(KeymapError::ConflictingBinding { .. })
        ));
    }

    proptest! {
        #[test]
        fn push_then_pop_preserves_existing_layers(raw in prop::collection::vec(0u8..26, 1..10)) {
            let mut stack = KeymapStack::new();
            let base_id = stack.push(layer("base", vec![binding('a', "base")])).unwrap();
            let before = stack.layers().to_vec();

            let bindings = raw
                .iter()
                .enumerate()
                .map(|(index, value)| {
                    let key = char::from(b'a' + *value);
                    Binding::new(
                        Sequence::single(Key::new(
                            KeyCode::Character(key),
                            Modifiers { shift: index % 2 == 1, ..Modifiers::default() },
                        )),
                        Action::named(format!("action-{index}")),
                        format!("Action {index}"),
                    )
                })
                .collect::<Vec<_>>();

            let id = stack.push(layer("generated", bindings)).ok();
            if let Some(id) = id {
                stack.pop(id);
                prop_assert_eq!(stack.layers(), before.as_slice());
            }
            prop_assert_eq!(stack.layers()[0].id(), base_id);
        }

        #[test]
        fn dispatch_is_deterministic(key in 0u8..26) {
            let key = char::from(b'a' + key);
            let mut stack = KeymapStack::new();
            stack.push(layer("list", vec![binding(key, "action")])).unwrap();
            let sequence = Sequence::single(Key::character(key));

            prop_assert_eq!(stack.dispatch(&sequence), stack.dispatch(&sequence));
        }
    }
}
