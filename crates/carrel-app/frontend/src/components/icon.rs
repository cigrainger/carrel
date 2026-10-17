//! Shared Lucide vocabulary for navigation and reading actions.

use leptos::prelude::*;
use lucide_leptos as lucide;

#[derive(Clone, Copy)]
pub(crate) enum IconName {
    Today,
    Reading,
    Library,
    Highlights,
    Refresh,
    Add,
    Close,
    Devices,
    Import,
    Export,
    Read,
    Star,
    Back,
    Previous,
    Next,
    SidebarOpen,
    SidebarClose,
    Info,
    Original,
    Search,
    Keyboard,
    Settings,
    More,
    Share,
    People,
    NewFolder,
}

/// Decorative icon; the containing control supplies its accessible name.
#[component]
pub(crate) fn Icon(name: IconName) -> impl IntoView {
    let icon = match name {
        IconName::Today => view! { <lucide::CalendarDays size=18/> }.into_any(),
        IconName::Reading => view! { <lucide::BookOpen size=18/> }.into_any(),
        IconName::Library => view! { <lucide::Library size=18/> }.into_any(),
        IconName::Highlights => view! { <lucide::Highlighter size=18/> }.into_any(),
        IconName::Refresh => view! { <lucide::RefreshCw size=16/> }.into_any(),
        IconName::Add => view! { <lucide::Plus size=16/> }.into_any(),
        IconName::Close => view! { <lucide::X size=16/> }.into_any(),
        IconName::Devices => view! { <lucide::Monitor size=16/> }.into_any(),
        IconName::Import => view! { <lucide::ArrowDownToLine size=16/> }.into_any(),
        IconName::Export => view! { <lucide::ArrowUpFromLine size=16/> }.into_any(),
        IconName::Read => view! { <lucide::CircleCheck size=18/> }.into_any(),
        IconName::Star => view! { <lucide::Star size=18/> }.into_any(),
        IconName::Back => view! { <lucide::ArrowLeft size=18/> }.into_any(),
        IconName::Previous => view! { <lucide::ChevronLeft size=18/> }.into_any(),
        IconName::Next => view! { <lucide::ChevronRight size=18/> }.into_any(),
        IconName::SidebarOpen => view! { <lucide::PanelLeftOpen size=18/> }.into_any(),
        IconName::SidebarClose => view! { <lucide::PanelLeftClose size=18/> }.into_any(),
        IconName::Info => view! { <lucide::Info size=18/> }.into_any(),
        IconName::Original => view! { <lucide::ExternalLink size=18/> }.into_any(),
        IconName::Search => view! { <lucide::Search size=16/> }.into_any(),
        IconName::Keyboard => view! { <lucide::Keyboard size=16/> }.into_any(),
        IconName::Settings => view! { <lucide::Settings size=16/> }.into_any(),
        IconName::More => view! { <lucide::Ellipsis size=18/> }.into_any(),
        IconName::Share => view! { <lucide::Share2 size=18/> }.into_any(),
        IconName::People => view! { <lucide::Users size=16/> }.into_any(),
        IconName::NewFolder => view! { <lucide::FolderPlus size=16/> }.into_any(),
    };
    view! { <span class="icon" aria-hidden="true">{icon}</span> }
}
