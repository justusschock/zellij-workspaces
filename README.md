# zellij-workspaces

Persistent Zellij workspaces with a responsive session sidebar, bottom tab
bar, and runtime-rendered workspace templates.

Workspace templates are ordinary Zellij KDL with MiniJinja placeholders. They
are discovered and rendered at runtime, so adding or editing one never
requires rebuilding the runner or plugin.

## Screenshots

The responsive workspace dashboard keeps sessions visible in the left sidebar,
tabs along the bottom, and the rest of the terminal available for real work:

![A Zellij workspace with a session sidebar, editor and checks panes, and a compact bottom tab bar](docs/images/workspace-dashboard.png)

Workspace layouts are discovered from the templates directory at runtime, so
the creation flow immediately offers new or edited templates:

![The runtime workspace template selector offering default, development, and services layouts](docs/images/template-selector.png)

## Build and install

Requirements: stable Rust, Zellij, and the `wasm32-wasip1` Rust target.

```sh
rustup target add wasm32-wasip1
cargo install --path crates/runner
cargo build --release --package zellij-session-ui --target wasm32-wasip1
mkdir -p ~/.config/zellij/plugins ~/.config/zellij-workspaces/templates
cp target/wasm32-wasip1/release/session-ui.wasm \
  ~/.config/zellij/plugins/session-ui.wasm
cp examples/*.kdl.tmpl ~/.config/zellij-workspaces/templates/
```

Register the plugin in `~/.config/zellij/config.kdl`:

```kdl
plugins {
    session-ui location="file:~/.config/zellij/plugins/session-ui.wasm"
}
```

Run `zellij-workspaces` to select or create a workspace. Run
`zellij-workspaces --new` to open the creation flow directly.

## Template contract

Templates live in `~/.config/zellij-workspaces/templates` by default and end
in `.kdl.tmpl`. The following values are available:

- `workspace.name`: the requested Zellij session name
- `workspace.cwd`: the selected absolute working directory
- `tools.shell`: `$SHELL`, falling back to `/bin/sh`
- `tools.editor`: `$VISUAL`, then `$EDITOR`, falling back to `vi`
- `vars`: environment variables explicitly prefixed with
  `ZELLIJ_WORKSPACES_VAR_`

Use the `kdl` filter whenever a value is placed inside a quoted KDL string:

```kdl
pane cwd="{{ workspace.cwd | kdl }}"
```

For example, `ZELLIJ_WORKSPACES_VAR_AGENT=codex` is available as
`vars.AGENT`. Unprefixed environment variables are not exposed to templates.

Rendered layouts are written atomically to the platform cache directory and
passed to Zellij by absolute path. Templates themselves are never modified.
Different rendered contents get different cache files, so concurrent workspace
creation cannot overwrite another workspace's layout.

Templates and their rendered layouts are configuration files, not a secret
store. Do not interpolate credentials. Resolve secrets inside the launched
process through your normal secret manager instead.

Render a template without starting Zellij:

```sh
zellij-workspaces --render development demo "$PWD"
```

The command prints the generated layout path, which is useful for inspection
and validation.

## Examples

`examples/development.kdl.tmpl` demonstrates editor, agent, and shell tabs.
`examples/services.kdl.tmpl` demonstrates generic service and log panes. They
contain no project-specific services, hosts, ports, or vendor assumptions.

## Configuration

The runner supports these environment variables:

- `ZELLIJ_WORKSPACES_ROOT_DIR`
- `ZELLIJ_WORKSPACES_IGNORE_DIRS`
- `ZELLIJ_WORKSPACES_MAX_DIRS_DEPTH`
- `ZELLIJ_WORKSPACES_TEMPLATES_DIR`
- `ZELLIJ_WORKSPACES_CACHE_DIR`
- `ZELLIJ_WORKSPACES_BANNERS_DIR`

Relative paths are resolved from the home directory.

## Attribution

This project builds on Alex Fedoseev's Zellij runner and statusbar work. See
[NOTICE.md](NOTICE.md) and the [upstream permission](https://github.com/alex35mil/dotfiles/issues/11#issuecomment-5233032745).

## License

MIT
