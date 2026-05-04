#![deny(unsafe_code)]

#[cfg(target_arch = "wasm32")]
fn main() {
    use carrel_app_frontend::App;
    use leptos::mount::mount_to_body;

    mount_to_body(App);
}

#[cfg(not(target_arch = "wasm32"))]
fn main() {}
