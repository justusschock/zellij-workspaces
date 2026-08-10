//! Runtime workspace templates and persistent session switching for Zellij.
//!
//! Run `zellij-workspaces` to switch sessions or create a workspace through
//! the interactive flow. Run `zellij-workspaces --new` to open the creation
//! flow directly. Templates are discovered from
//! `~/.config/zellij-workspaces/templates` unless overridden.

mod action;
mod dir;
mod log;
mod options;
mod runner;
mod template;
mod ui;
mod zellij;

fn main() {
    runner::init();

    loop {
        runner::switch();
    }
}
