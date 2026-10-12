//! Shared application chrome components.

pub(crate) mod devices;
mod feed_activity;
mod icon;
mod item_row;
mod modal;
pub(crate) mod save_article;
pub(crate) mod settings;
mod sidebar;
mod status_strip;

pub(crate) use feed_activity::FeedActivity;
pub(crate) use feed_activity::SourceActivityContent;
pub(crate) use icon::{Icon, IconName};
pub use item_row::ItemList;
pub(crate) use modal::Modal;
pub use sidebar::Sidebar;
pub(crate) use sidebar::{ContextControls, provide_context_bar};
pub(crate) use sidebar::{FieldNavigation, SubscriptionFiles};
pub use status_strip::StatusStrip;
pub(crate) use status_strip::{AppearanceMenu, ThemeControls};
