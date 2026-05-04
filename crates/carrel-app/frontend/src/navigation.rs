//! Shared route-local navigation context.

use leptos::prelude::*;

/// The list context that opened an item.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ListNavigation {
    pub item_ids: Vec<String>,
    pub current_id: String,
    pub return_path: String,
}

/// Reading-route navigation state shared through Leptos context.
#[derive(Clone, Copy)]
pub struct ListNavigationContext {
    current: RwSignal<Option<ListNavigation>>,
}

impl ListNavigationContext {
    pub fn set(&self, navigation: ListNavigation) {
        self.current.set(Some(navigation));
    }

    pub fn get(&self) -> Option<ListNavigation> {
        self.current.get_untracked()
    }
}

/// Provide list navigation context for route children.
pub fn provide_list_navigation_context() -> ListNavigationContext {
    let context = ListNavigationContext {
        current: RwSignal::new(None),
    };
    provide_context(context);
    context
}

/// Read the current list navigation context.
pub fn use_list_navigation_context() -> ListNavigationContext {
    use_context::<ListNavigationContext>().expect("ListNavigationContext should be mounted")
}
