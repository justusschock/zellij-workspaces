use std::{collections::BTreeSet, fs, io, path::Path};

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use kdl::{KdlDocument, KdlNode};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum PickerAction {
    Up,
    Down,
    Accept,
    Cancel,
    Quit,
    NewWorkspace,
    Yes,
    No,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct Binding {
    code: KeyCode,
    modifiers: KeyModifiers,
}

impl Binding {
    fn parse(value: &str) -> io::Result<Self> {
        let mut parts = value.split_ascii_whitespace().collect::<Vec<_>>();
        let key = parts
            .pop()
            .ok_or_else(|| invalid(format!("empty key binding `{value}`")))?;
        let mut modifiers = KeyModifiers::NONE;
        for modifier in parts {
            modifiers |= match modifier.to_ascii_lowercase().as_str() {
                "ctrl" => KeyModifiers::CONTROL,
                "alt" => KeyModifiers::ALT,
                "shift" => KeyModifiers::SHIFT,
                "super" => KeyModifiers::SUPER,
                _ => return Err(invalid(format!("unsupported modifier `{modifier}`"))),
            };
        }

        let mut code = parse_key_code(key)?;
        if let KeyCode::Char(character) = code
            && character.is_ascii_uppercase()
        {
            code = KeyCode::Char(character.to_ascii_lowercase());
            modifiers |= KeyModifiers::SHIFT;
        }

        Ok(Self { code, modifiers })
    }

    fn matches(&self, event: &KeyEvent) -> bool {
        self.code == event.code && self.modifiers == event.modifiers
    }

    fn canonical(&self) -> String {
        let mut parts = Vec::new();
        for (modifier, name) in [
            (KeyModifiers::CONTROL, "Ctrl"),
            (KeyModifiers::ALT, "Alt"),
            (KeyModifiers::SHIFT, "Shift"),
            (KeyModifiers::SUPER, "Super"),
        ] {
            if self.modifiers.contains(modifier) {
                parts.push(name.to_owned());
            }
        }
        parts.push(format_key_code(self.code));
        parts.join(" ")
    }
}

#[derive(Clone, Debug)]
pub(crate) struct PickerKeymaps {
    up: Vec<Binding>,
    down: Vec<Binding>,
    accept: Vec<Binding>,
    cancel: Vec<Binding>,
    quit: Vec<Binding>,
    new_workspace: Vec<Binding>,
    yes: Vec<Binding>,
    no: Vec<Binding>,
}

impl PickerKeymaps {
    pub(crate) fn action(&self, event: &KeyEvent) -> Option<PickerAction> {
        [
            (PickerAction::Up, &self.up),
            (PickerAction::Down, &self.down),
            (PickerAction::Accept, &self.accept),
            (PickerAction::Cancel, &self.cancel),
            (PickerAction::Quit, &self.quit),
            (PickerAction::NewWorkspace, &self.new_workspace),
            (PickerAction::Yes, &self.yes),
            (PickerAction::No, &self.no),
        ]
        .into_iter()
        .find_map(|(action, bindings)| {
            bindings
                .iter()
                .any(|binding| binding.matches(event))
                .then_some(action)
        })
    }

    fn entries(&self) -> [(&'static str, &Vec<Binding>); 8] {
        [
            ("up", &self.up),
            ("down", &self.down),
            ("accept", &self.accept),
            ("cancel", &self.cancel),
            ("quit", &self.quit),
            ("new_workspace", &self.new_workspace),
            ("yes", &self.yes),
            ("no", &self.no),
        ]
    }
}

impl Default for PickerKeymaps {
    fn default() -> Self {
        Self {
            up: bindings(&["Up"]),
            down: bindings(&["Down"]),
            accept: bindings(&["Enter"]),
            cancel: bindings(&["Esc"]),
            quit: bindings(&["Ctrl c"]),
            new_workspace: bindings(&["Ctrl n"]),
            yes: bindings(&["y"]),
            no: bindings(&["n"]),
        }
    }
}

#[derive(Clone, Debug)]
struct SidebarKeymaps {
    up: Vec<Binding>,
    down: Vec<Binding>,
    open: Vec<Binding>,
    new_workspace: Vec<Binding>,
    cancel: Vec<Binding>,
}

impl SidebarKeymaps {
    fn entries(&self) -> [(&'static str, &Vec<Binding>); 5] {
        [
            ("up", &self.up),
            ("down", &self.down),
            ("open", &self.open),
            ("new_workspace", &self.new_workspace),
            ("cancel", &self.cancel),
        ]
    }
}

impl Default for SidebarKeymaps {
    fn default() -> Self {
        Self {
            up: bindings(&["k", "Up"]),
            down: bindings(&["j", "Down"]),
            open: bindings(&["Enter"]),
            new_workspace: bindings(&["n"]),
            cancel: bindings(&["Esc"]),
        }
    }
}

#[derive(Clone, Debug, Default)]
pub(crate) struct Keymaps {
    pub(crate) picker: PickerKeymaps,
    sidebar: SidebarKeymaps,
}

impl Keymaps {
    pub(crate) fn load(path: &Path) -> io::Result<Self> {
        match fs::read_to_string(path) {
            Ok(source) => Self::parse(&source),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(Self::default()),
            Err(error) => Err(error),
        }
    }

    fn parse(source: &str) -> io::Result<Self> {
        let document = source
            .parse::<KdlDocument>()
            .map_err(|error| invalid(format!("invalid keymap KDL: {error}")))?;
        if document.nodes().len() != 1 || document.nodes()[0].name().value() != "keymaps" {
            return Err(invalid(
                "keymaps.kdl must contain exactly one top-level `keymaps` node",
            ));
        }
        let root = document
            .get("keymaps")
            .ok_or_else(|| invalid("keymaps.kdl must contain a `keymaps` node"))?;
        if !root.entries().is_empty() {
            return Err(invalid(
                "the `keymaps` node cannot have arguments or properties",
            ));
        }
        let sections = root
            .children()
            .ok_or_else(|| invalid("the `keymaps` node must contain `picker` and `sidebar`"))?;

        let mut keymaps = Self::default();
        let mut seen_sections = BTreeSet::new();
        for section in sections.nodes() {
            let name = section.name().value();
            if !seen_sections.insert(name.to_owned()) {
                return Err(invalid(format!("duplicate keymap section `{name}`")));
            }
            match name {
                "picker" => parse_picker(section, &mut keymaps.picker)?,
                "sidebar" => parse_sidebar(section, &mut keymaps.sidebar)?,
                _ => return Err(invalid(format!("unknown keymap section `{name}`"))),
            }
        }
        validate_unique("picker", keymaps.picker.entries())?;
        validate_unique("sidebar", keymaps.sidebar.entries())?;
        Ok(keymaps)
    }

    pub(crate) fn sidebar_protocol(&self) -> String {
        let mut output = String::new();
        for (action, bindings) in self.sidebar.entries() {
            for binding in bindings {
                output.push_str(action);
                output.push('\t');
                output.push_str(&binding.canonical());
                output.push('\n');
            }
        }
        output
    }
}

fn parse_picker(section: &KdlNode, keymaps: &mut PickerKeymaps) -> io::Result<()> {
    if !section.entries().is_empty() {
        return Err(invalid(
            "the `picker` keymap section cannot have arguments or properties",
        ));
    }
    let actions = section
        .children()
        .ok_or_else(|| invalid("the `picker` keymap section must contain actions"))?;
    let mut seen = BTreeSet::new();
    for node in actions.nodes() {
        let name = node.name().value();
        if !seen.insert(name.to_owned()) {
            return Err(invalid(format!("duplicate picker action `{name}`")));
        }
        let parsed = node_bindings("picker", node)?;
        match name {
            "up" => keymaps.up = parsed,
            "down" => keymaps.down = parsed,
            "accept" => keymaps.accept = parsed,
            "cancel" => keymaps.cancel = parsed,
            "quit" => keymaps.quit = parsed,
            "new_workspace" => keymaps.new_workspace = parsed,
            "yes" => keymaps.yes = parsed,
            "no" => keymaps.no = parsed,
            _ => return Err(invalid(format!("unknown picker action `{name}`"))),
        }
    }
    Ok(())
}

fn parse_sidebar(section: &KdlNode, keymaps: &mut SidebarKeymaps) -> io::Result<()> {
    if !section.entries().is_empty() {
        return Err(invalid(
            "the `sidebar` keymap section cannot have arguments or properties",
        ));
    }
    let actions = section
        .children()
        .ok_or_else(|| invalid("the `sidebar` keymap section must contain actions"))?;
    let mut seen = BTreeSet::new();
    for node in actions.nodes() {
        let name = node.name().value();
        if !seen.insert(name.to_owned()) {
            return Err(invalid(format!("duplicate sidebar action `{name}`")));
        }
        let parsed = node_bindings("sidebar", node)?;
        match name {
            "up" => keymaps.up = parsed,
            "down" => keymaps.down = parsed,
            "open" => keymaps.open = parsed,
            "new_workspace" => keymaps.new_workspace = parsed,
            "cancel" => keymaps.cancel = parsed,
            _ => return Err(invalid(format!("unknown sidebar action `{name}`"))),
        }
    }
    Ok(())
}

fn node_bindings(section: &str, node: &KdlNode) -> io::Result<Vec<Binding>> {
    if node.children().is_some() || node.entries().iter().any(|entry| entry.name().is_some()) {
        return Err(invalid(format!(
            "{section}.{} must contain only string arguments",
            node.name().value()
        )));
    }
    let values = node
        .entries()
        .iter()
        .map(|entry| {
            entry.value().as_string().ok_or_else(|| {
                invalid(format!(
                    "{section}.{} bindings must be strings",
                    node.name().value()
                ))
            })
        })
        .collect::<io::Result<Vec<_>>>()?;
    if values.is_empty() {
        return Err(invalid(format!(
            "{section}.{} must have at least one binding",
            node.name().value()
        )));
    }
    values.into_iter().map(Binding::parse).collect()
}

fn validate_unique<const N: usize>(
    section: &str,
    actions: [(&'static str, &Vec<Binding>); N],
) -> io::Result<()> {
    let mut seen = std::collections::BTreeMap::<String, &'static str>::new();
    for (action, bindings) in actions {
        for binding in bindings {
            let key = binding.canonical();
            if let Some(previous) = seen.insert(key.clone(), action) {
                return Err(invalid(format!(
                    "{section} binding `{key}` is assigned to both `{previous}` and `{action}`"
                )));
            }
        }
    }
    Ok(())
}

fn parse_key_code(key: &str) -> io::Result<KeyCode> {
    let code = match key.to_ascii_lowercase().as_str() {
        "pagedown" | "pgdn" => KeyCode::PageDown,
        "pageup" | "pgup" => KeyCode::PageUp,
        "left" => KeyCode::Left,
        "down" => KeyCode::Down,
        "up" => KeyCode::Up,
        "right" => KeyCode::Right,
        "home" => KeyCode::Home,
        "end" => KeyCode::End,
        "backspace" => KeyCode::Backspace,
        "delete" => KeyCode::Delete,
        "insert" => KeyCode::Insert,
        "tab" => KeyCode::Tab,
        "esc" => KeyCode::Esc,
        "enter" => KeyCode::Enter,
        "space" => KeyCode::Char(' '),
        value if value.starts_with('f') => {
            let number = value[1..]
                .parse::<u8>()
                .map_err(|_| invalid(format!("unsupported key `{key}`")))?;
            if !(1..=12).contains(&number) {
                return Err(invalid(format!("unsupported key `{key}`")));
            }
            KeyCode::F(number)
        }
        _ if key.chars().count() == 1 => KeyCode::Char(key.chars().next().unwrap()),
        _ => return Err(invalid(format!("unsupported key `{key}`"))),
    };
    Ok(code)
}

fn format_key_code(code: KeyCode) -> String {
    match code {
        KeyCode::PageDown => "PageDown".into(),
        KeyCode::PageUp => "PageUp".into(),
        KeyCode::Left => "Left".into(),
        KeyCode::Down => "Down".into(),
        KeyCode::Up => "Up".into(),
        KeyCode::Right => "Right".into(),
        KeyCode::Home => "Home".into(),
        KeyCode::End => "End".into(),
        KeyCode::Backspace => "Backspace".into(),
        KeyCode::Delete => "Delete".into(),
        KeyCode::Insert => "Insert".into(),
        KeyCode::F(number) => format!("F{number}"),
        KeyCode::Char(' ') => "Space".into(),
        KeyCode::Char(character) => character.to_string(),
        KeyCode::Tab => "Tab".into(),
        KeyCode::Esc => "Esc".into(),
        KeyCode::Enter => "Enter".into(),
        _ => unreachable!("unsupported key code cannot be constructed"),
    }
}

fn bindings(values: &[&str]) -> Vec<Binding> {
    values
        .iter()
        .map(|value| Binding::parse(value).expect("default key binding must be valid"))
        .collect()
}

fn invalid(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message.into())
}

#[cfg(test)]
mod tests {
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    use super::{Keymaps, PickerAction};

    #[test]
    fn missing_file_uses_defaults() {
        let keymaps = Keymaps::load(Path::new("/definitely/missing/keymaps.kdl")).unwrap();
        assert_eq!(
            keymaps
                .picker
                .action(&KeyEvent::new(KeyCode::Down, KeyModifiers::NONE)),
            Some(PickerAction::Down)
        );
    }

    #[test]
    fn kdl_overrides_only_named_actions() {
        let keymaps = Keymaps::parse(
            r#"
            keymaps {
                picker { down "Ctrl j"; }
                sidebar { up "w"; open "Space"; }
            }
            "#,
        )
        .unwrap();

        assert_eq!(
            keymaps
                .picker
                .action(&KeyEvent::new(KeyCode::Char('j'), KeyModifiers::CONTROL)),
            Some(PickerAction::Down)
        );
        assert_eq!(
            keymaps
                .picker
                .action(&KeyEvent::new(KeyCode::Down, KeyModifiers::NONE)),
            None
        );
        assert!(keymaps.sidebar_protocol().contains("up\tw\n"));
        assert!(keymaps.sidebar_protocol().contains("open\tSpace\n"));
        assert!(keymaps.sidebar_protocol().contains("down\tj\n"));
    }

    #[test]
    fn invalid_and_duplicate_actions_are_rejected() {
        assert!(
            Keymaps::parse(
                r#"
                keymaps {
                    sidebar {
                        unknown "x"
                    }
                }
                "#,
            )
            .unwrap_err()
            .to_string()
            .contains("unknown sidebar action")
        );
        assert!(
            Keymaps::parse(
                r#"
                keymaps {
                    picker {
                        up "k"
                        up "Up"
                    }
                }
                "#,
            )
            .unwrap_err()
            .to_string()
            .contains("duplicate picker action")
        );
        assert!(
            Keymaps::parse(
                r#"
                keymaps {
                    sidebar {
                        up "g"
                        down "g"
                    }
                }
                "#,
            )
            .unwrap_err()
            .to_string()
            .contains("assigned to both")
        );
    }

    use std::path::Path;
}
