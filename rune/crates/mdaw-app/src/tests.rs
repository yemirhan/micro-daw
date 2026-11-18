//! Element-tree / `Runtime` tests (phase 7): every interaction of
//! `docs/rune-port/inventory/beat-ui.md`, driven through the real view with pointer, wheel and
//! key events, asserting the engine / doc state the TS asserts (the table in
//! `docs/rune-port/phase-7-beat-ui.md` maps each checklist line to a test).

mod harness;
mod t_a11y;
mod t_browser;
mod t_editor;
mod t_lifecycle;
mod t_menus;
mod t_pads;
mod t_shell;
mod t_steps;
mod t_transport;
