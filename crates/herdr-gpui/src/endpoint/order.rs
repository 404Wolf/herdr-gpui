//! The order the window lists devices in. The catalogs list them as each was
//! saved: SSH hosts as upstream keeps them, then WSL distributions, then
//! cloud devices. Dragging a device's header reorders them for this client
//! only, by endpoint ID, so upstream's catalog is never rewritten and a
//! device saved later simply lists after the ones the user arranged. Local
//! stays first: much of the window knows it as endpoint 0.

use super::LOCAL;
use crate::{HerdrWindow, reorder::Beside};

/// Where `id` sorts under the user's `order`: its place there, or after
/// every arranged device, so a stable sort keeps the catalog's own order
/// for the rest.
pub(super) fn rank(order: &[String], id: &str) -> usize {
    order
        .iter()
        .position(|arranged| arranged == id)
        .unwrap_or(order.len())
}

/// The device order after moving `moved` beside an anchor, from `ids`, the
/// devices in their current order. `None` when either is not listed.
fn moved(ids: &[&str], moved: &str, to: Beside<&str>) -> Option<Vec<String>> {
    let mut order: Vec<_> = ids.iter().copied().filter(|id| *id != moved).collect();
    if order.len() == ids.len() {
        return None;
    }
    let at = |anchor: &str| order.iter().position(|id| *id == anchor);
    let index = match to {
        Beside::Before(anchor) => at(anchor)?,
        Beside::After(anchor) => at(anchor)? + 1,
    };
    order.insert(index, moved);
    Some(order.into_iter().map(str::to_owned).collect())
}

impl HerdrWindow {
    /// Lists the devices in the user's order, keeping the selection on the
    /// device it was on.
    pub(crate) fn arrange_devices(&mut self) {
        let selected = self.endpoints[self.selected_endpoint].id.clone();
        let order = &self.device_order;
        self.endpoints[1..].sort_by_key(|endpoint| rank(order, &endpoint.id));
        self.selected_endpoint = self
            .endpoints
            .iter()
            .position(|endpoint| endpoint.id == selected)
            .unwrap_or(0);
    }

    /// Moves device `id` beside another and remembers the new order. Returns
    /// whether anything moved.
    pub(crate) fn move_device(&mut self, id: &str, to: Beside<&str>) -> bool {
        if id == LOCAL {
            return false;
        }
        let ids: Vec<_> = self.endpoints[1..]
            .iter()
            .map(|endpoint| endpoint.id.as_str())
            .collect();
        let Some(order) = moved(&ids, id, to) else {
            return false;
        };
        if order.iter().map(String::as_str).eq(ids.iter().copied()) {
            return false;
        }
        self.device_order = order;
        self.arrange_devices();
        self.save_chrome();
        true
    }
}

#[cfg(test)]
mod tests;
