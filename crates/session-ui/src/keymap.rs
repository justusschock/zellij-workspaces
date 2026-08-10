use std::{collections::BTreeMap, str::FromStr};

use zellij_tile::prelude::KeyWithModifier;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum SidebarAction {
    Up,
    Down,
    Open,
    NewWorkspace,
    Cancel,
}

#[derive(Clone, Debug)]
pub(crate) struct SidebarKeymaps {
    up: Vec<KeyWithModifier>,
    down: Vec<KeyWithModifier>,
    open: Vec<KeyWithModifier>,
    new_workspace: Vec<KeyWithModifier>,
    cancel: Vec<KeyWithModifier>,
}

impl SidebarKeymaps {
    pub(crate) fn from_protocol(source: &str) -> Result<Self, String> {
        let mut actions = BTreeMap::<&str, Vec<KeyWithModifier>>::new();
        for (index, line) in source.lines().enumerate() {
            if line.is_empty() {
                continue;
            }
            let (action, key) = line
                .split_once('\t')
                .ok_or_else(|| format!("invalid sidebar keymap line {}: missing tab", index + 1))?;
            if !matches!(action, "up" | "down" | "open" | "new_workspace" | "cancel") {
                return Err(format!("unknown sidebar keymap action `{action}`"));
            }
            let key = KeyWithModifier::from_str(key)
                .map_err(|error| format!("invalid sidebar key `{key}`: {error}"))?;
            actions.entry(action).or_default().push(key);
        }

        Ok(Self {
            up: take(&mut actions, "up")?,
            down: take(&mut actions, "down")?,
            open: take(&mut actions, "open")?,
            new_workspace: take(&mut actions, "new_workspace")?,
            cancel: take(&mut actions, "cancel")?,
        })
    }

    pub(crate) fn action(&self, key: &KeyWithModifier) -> Option<SidebarAction> {
        [
            (SidebarAction::Up, &self.up),
            (SidebarAction::Down, &self.down),
            (SidebarAction::Open, &self.open),
            (SidebarAction::NewWorkspace, &self.new_workspace),
            (SidebarAction::Cancel, &self.cancel),
        ]
        .into_iter()
        .find_map(|(action, bindings)| bindings.contains(key).then_some(action))
    }
}

impl Default for SidebarKeymaps {
    fn default() -> Self {
        Self::from_protocol(
            "up\tk\nup\tUp\ndown\tj\ndown\tDown\nopen\tEnter\nnew_workspace\tn\ncancel\tEsc\n",
        )
        .expect("default sidebar keymaps must be valid")
    }
}

fn take(
    actions: &mut BTreeMap<&str, Vec<KeyWithModifier>>,
    name: &'static str,
) -> Result<Vec<KeyWithModifier>, String> {
    actions
        .remove(name)
        .filter(|bindings| !bindings.is_empty())
        .ok_or_else(|| format!("sidebar keymap action `{name}` has no bindings"))
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use zellij_tile::prelude::KeyWithModifier;

    use super::{SidebarAction, SidebarKeymaps};

    #[test]
    fn protocol_replaces_defaults_and_supports_modifiers() {
        let keymaps = SidebarKeymaps::from_protocol(
            "up\tCtrl k\ndown\tCtrl j\nopen\tSpace\nnew_workspace\ta\ncancel\tEsc\n",
        )
        .unwrap();

        assert_eq!(
            keymaps.action(&KeyWithModifier::from_str("Ctrl k").unwrap()),
            Some(SidebarAction::Up)
        );
        assert_eq!(
            keymaps.action(&KeyWithModifier::from_str("k").unwrap()),
            None
        );
        assert_eq!(
            keymaps.action(&KeyWithModifier::from_str("Space").unwrap()),
            Some(SidebarAction::Open)
        );
    }

    #[test]
    fn incomplete_protocol_is_rejected() {
        assert!(
            SidebarKeymaps::from_protocol("up\tk\n")
                .unwrap_err()
                .contains("down")
        );
    }
}
