//! Agents listed under the workspace they run in, for
//! `agents_placement = "nested"`: the same rows as the agents section, a
//! step further in than their workspace, named by the agent alone since the
//! row above names the workspace.

use crate::{
    HerdrWindow,
    sidebar::{agents::sorted_agents, cell::RowContext},
};
use gpui::{prelude::*, *};
use herdr_client::protocol::ClientShellSnapshot;

impl HerdrWindow {
    /// Rows for endpoint `index`'s agents in `workspace_id`, in the panel's
    /// order, indented `indent` past the workspace row drawn in `row_cx`.
    pub(in crate::sidebar) fn nested_agent_rows(
        &self,
        index: usize,
        snapshot: &ClientShellSnapshot,
        workspace_id: &str,
        (row_cx, indent): (&RowContext, f32),
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        let row_cx = RowContext {
            nest: row_cx.nest + indent,
            host: None,
            ..*row_cx
        };
        sorted_agents(snapshot, self.agent_sort)
            .into_iter()
            .filter(|agent| agent.workspace_id == workspace_id)
            .filter_map(|agent| {
                let lines = self.configured_agent_lines(agent, snapshot, None)?;
                let row = self.agent_cell(index, agent, snapshot, &row_cx, lines, 0., true, cx);
                Some(row.into_any_element())
            })
            .collect()
    }
}
