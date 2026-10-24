//! The order the sidebar lists workspaces and agents in, which the previous,
//! next, and numbered shortcuts step through, as Herdr's TUI steps through
//! its own sidebar: every listed host's rows, top to bottom.

use super::{agents::panel_agents, visible_workspace_entries, workspaces::grouped_keys};
use crate::{HerdrWindow, controls::Command, state::LiveState};
use herdr_client::protocol::{ClientShellAgent, ClientShellSnapshot};

/// A listed row: its host, by endpoint index, and the workspace or agent
/// pane it shows.
pub(crate) type Row<'a> = (usize, &'a str);

impl HerdrWindow {
    /// Each host the sidebar lists, with the live state it draws from.
    fn listed_lives(&self) -> impl Iterator<Item = (usize, &LiveState)> {
        self.displayed_devices()
            .into_iter()
            .filter(|(_, endpoint)| endpoint.enabled && self.device_visible(&endpoint.id))
            .map(|(index, endpoint)| {
                let live = if index == self.selected_endpoint {
                    &self.live
                } else {
                    &endpoint.live
                };
                (index, live)
            })
    }

    /// Each host the sidebar lists, with the snapshot it draws from.
    fn listed_hosts(&self) -> impl Iterator<Item = (usize, &ClientShellSnapshot)> {
        self.listed_lives()
            .filter_map(|(index, live)| live.snapshot.as_deref().map(|snapshot| (index, snapshot)))
    }

    /// The workspace rows the sidebar shows: a folded group or host hides
    /// its rows here too.
    pub(crate) fn sidebar_workspaces(&self) -> Vec<Row<'_>> {
        let multi = self.endpoints.len() > 1;
        self.listed_hosts()
            .filter(|(index, _)| !(multi && self.endpoints[*index].collapsed))
            .flat_map(|(index, snapshot)| {
                let collapsed = if index == 0 {
                    &self.collapsed_repos
                } else {
                    &self.endpoints[index].collapsed_repos
                };
                visible_workspace_entries(&snapshot.workspaces, collapsed)
                    .into_iter()
                    .map(move |(row, _, _)| (index, snapshot.workspaces[row].workspace_id.as_str()))
            })
            .collect()
    }

    /// For each row `sidebar_workspaces` returns, whether it starts a group:
    /// a repository with its worktrees, or a workspace on its own. A group
    /// never spans two hosts.
    pub(crate) fn sidebar_group_starts(&self) -> Vec<bool> {
        let multi = self.endpoints.len() > 1;
        self.listed_hosts()
            .filter(|(index, _)| !(multi && self.endpoints[*index].collapsed))
            .flat_map(|(index, snapshot)| {
                let collapsed = if index == 0 {
                    &self.collapsed_repos
                } else {
                    &self.endpoints[index].collapsed_repos
                };
                let grouped = grouped_keys(&snapshot.workspaces);
                let mut last: Option<&str> = None;
                visible_workspace_entries(&snapshot.workspaces, collapsed)
                    .into_iter()
                    .enumerate()
                    .map(move |(position, (row, _, _))| {
                        let key = snapshot.workspaces[row]
                            .worktree
                            .as_ref()
                            .map(|tree| tree.key.as_str())
                            .filter(|key| grouped.contains(key));
                        let start = position == 0 || key.is_none() || key != last;
                        last = key;
                        start
                    })
                    .collect::<Vec<_>>()
            })
            .collect()
    }

    /// The agents the panel paints, by host index, in its order. Painting
    /// and stepping both read it, so a shortcut lands on the row below.
    pub(super) fn panel_agents(&self) -> Vec<(usize, &ClientShellAgent)> {
        panel_agents(self.listed_lives(), self.agent_sort)
    }

    /// The agent panel's rows, whether or not the panel is shown, as Herdr
    /// steps through its agents either way.
    pub(crate) fn sidebar_agents(&self) -> Vec<Row<'_>> {
        self.panel_agents()
            .into_iter()
            .map(|(index, agent)| (index, agent.pane_id.as_str()))
            .collect()
    }
}

/// Which row a sidebar step lands on. `current` is the focused row, if the
/// sidebar lists it; `selected` is the selected host. Previous and next wrap
/// around, and start from an end when nothing listed is focused. A numbered
/// workspace counts only the selected host's rows and a numbered agent every
/// host's, as Herdr numbers them.
pub(crate) fn step(
    rows: &[Row<'_>],
    current: Option<usize>,
    selected: usize,
    command: Command,
) -> Option<usize> {
    let len = rows.len();
    if len == 0 {
        return None;
    }
    match command {
        Command::PreviousWorkspace | Command::PreviousAgent => {
            Some(current.map_or(len - 1, |index| (index + len - 1) % len))
        }
        Command::NextWorkspace | Command::NextAgent => {
            Some(current.map_or(0, |index| (index + 1) % len))
        }
        Command::WorkspaceNumber(number) => rows
            .iter()
            .enumerate()
            .filter(|(_, (host, _))| *host == selected)
            .nth(usize::from(number).checked_sub(1)?)
            .map(|(index, _)| index),
        Command::AgentNumber(number) => {
            let index = usize::from(number).checked_sub(1)?;
            (index < len).then_some(index)
        }
        _ => None,
    }
}

/// Which row a group step lands on: the first row of the group after, or
/// before, the focused row's, wrapping around. `starts` marks each row that
/// begins a group. With nothing focused, next takes the first group and
/// previous the last.
pub(crate) fn group_step(
    starts: &[bool],
    current: Option<usize>,
    command: Command,
) -> Option<usize> {
    let len = starts.len();
    if len == 0 {
        return None;
    }
    let start_at = |from: usize, forward: bool| {
        (1..=len)
            .map(|k| {
                if forward {
                    (from + k) % len
                } else {
                    (from + len - k) % len
                }
            })
            .find(|&index| starts[index])
    };
    match command {
        Command::NextGroup => match current {
            Some(current) => start_at(current, true),
            None => Some(0),
        },
        Command::PreviousGroup => {
            // From the focused row's own group start, or past the end.
            let from = match current {
                Some(current) => (0..=current)
                    .rev()
                    .find(|&index| starts[index])
                    .unwrap_or(0),
                None => 0,
            };
            start_at(from, false)
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn group_steps_land_on_group_starts_and_wrap() {
        // Groups: [0], [1, 2, 3], [4, 5].
        let starts = [true, true, false, false, true, false];
        for (command, current, expected) in [
            (Command::NextGroup, Some(0), Some(1)),
            (Command::NextGroup, Some(2), Some(4)),
            (Command::NextGroup, Some(5), Some(0)),
            (Command::NextGroup, None, Some(0)),
            (Command::PreviousGroup, Some(3), Some(0)),
            (Command::PreviousGroup, Some(1), Some(0)),
            (Command::PreviousGroup, Some(0), Some(4)),
            (Command::PreviousGroup, Some(5), Some(1)),
            (Command::PreviousGroup, None, Some(4)),
        ] {
            assert_eq!(
                group_step(&starts, current, command),
                expected,
                "{command:?} {current:?}"
            );
        }
        assert_eq!(group_step(&[], None, Command::NextGroup), None);
        assert_eq!(group_step(&starts, Some(0), Command::NextWorkspace), None);
    }

    #[test]
    fn steps_wrap_and_start_from_an_end() {
        let rows = [(0, "a"), (0, "b"), (1, "c")];
        for (command, current, expected) in [
            (Command::NextWorkspace, Some(0), Some(1)),
            (Command::NextWorkspace, Some(2), Some(0)),
            (Command::NextWorkspace, None, Some(0)),
            (Command::PreviousWorkspace, Some(0), Some(2)),
            (Command::PreviousWorkspace, Some(2), Some(1)),
            (Command::PreviousWorkspace, None, Some(2)),
            (Command::NextAgent, Some(1), Some(2)),
            (Command::PreviousAgent, None, Some(2)),
        ] {
            assert_eq!(step(&rows, current, 0, command), expected, "{command:?}");
        }
        for command in [Command::NextWorkspace, Command::PreviousAgent] {
            assert_eq!(step(&[], None, 0, command), None);
        }
        assert_eq!(step(&rows, Some(0), 0, Command::Tab), None);
    }

    #[test]
    fn numbers_count_from_one() {
        let rows = [(0, "a"), (1, "b"), (0, "c"), (1, "d")];
        // A numbered workspace counts the selected host's rows only.
        assert_eq!(step(&rows, None, 0, Command::WorkspaceNumber(2)), Some(2));
        assert_eq!(step(&rows, None, 1, Command::WorkspaceNumber(1)), Some(1));
        assert_eq!(step(&rows, None, 1, Command::WorkspaceNumber(3)), None);
        assert_eq!(step(&rows, None, 0, Command::WorkspaceNumber(0)), None);
        // A numbered agent counts every host's.
        assert_eq!(step(&rows, None, 0, Command::AgentNumber(4)), Some(3));
        assert_eq!(step(&rows, None, 0, Command::AgentNumber(5)), None);
        assert_eq!(step(&rows, None, 0, Command::AgentNumber(0)), None);
    }
}
