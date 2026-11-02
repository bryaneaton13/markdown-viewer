//! The sidebar's rows: every workspace as a folder that opens and closes,
//! with its file tree beneath it while open. Opening a folder here never
//! switches workspaces; opening a file does. Kept free of GPUI so ordering
//! and layout are unit-tested on plain data.

use serde::{Deserialize, Serialize};
use std::cmp::Reverse;

/// How folder workspaces are ordered. The loose "Open Files" workspace
/// always comes last.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Sort {
    /// The order they were added or dragged into.
    #[default]
    Manual,
    Name,
    /// Most recently active first.
    Recent,
}

impl Sort {
    pub const ALL: [Sort; 3] = [Sort::Manual, Sort::Name, Sort::Recent];

    pub fn label(self) -> &'static str {
        match self {
            Sort::Manual => "Manual (Drag to Reorder)",
            Sort::Name => "Name",
            Sort::Recent => "Recently Used",
        }
    }
}

/// What layout needs to know about one workspace.
#[derive(Clone, Debug, Default)]
pub struct Summary {
    pub name: String,
    /// A folder workspace, as opposed to loose files.
    pub folder: bool,
    pub open: bool,
    pub rows: usize,
    pub last_used: u64,
}

/// One sidebar row. Workspace indices are positions in the window's
/// workspace list, not in display order.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Item {
    /// A workspace's folder row.
    Header(usize),
    /// Row `row` of a workspace's file tree.
    Entry { ws: usize, row: usize },
    /// Placeholder under an open workspace with nothing to list.
    Empty(usize),
}

impl Item {
    pub fn ws(self) -> usize {
        match self {
            Item::Header(ws) | Item::Empty(ws) | Item::Entry { ws, .. } => ws,
        }
    }
}

/// Workspace indices in sidebar order.
pub fn order(workspaces: &[Summary], sort: Sort) -> Vec<usize> {
    let mut folders: Vec<usize> = (0..workspaces.len())
        .filter(|&i| workspaces[i].folder)
        .collect();
    match sort {
        Sort::Manual => {}
        Sort::Name => folders.sort_by_cached_key(|&i| workspaces[i].name.to_lowercase()),
        Sort::Recent => folders.sort_by_key(|&i| Reverse(workspaces[i].last_used)),
    }
    folders.extend((0..workspaces.len()).filter(|&i| !workspaces[i].folder));
    folders
}

/// Whether a workspace gets a row: loose files only once there are some.
pub fn visible(ws: &Summary) -> bool {
    ws.folder || ws.rows > 0
}

/// The rows to draw, top to bottom.
pub fn layout(workspaces: &[Summary], order: &[usize]) -> Vec<Item> {
    let mut items = Vec::new();
    for &ws in order {
        let summary = &workspaces[ws];
        if !visible(summary) {
            continue;
        }
        items.push(Item::Header(ws));
        if summary.open {
            if summary.rows == 0 {
                items.push(Item::Empty(ws));
            }
            items.extend((0..summary.rows).map(|row| Item::Entry { ws, row }));
        }
    }
    items
}

/// A new manual order after dragging workspace `from` onto workspace `to`:
/// it takes `to`'s place, and the rows between shift over.
pub fn reorder(order: &[usize], from: usize, to: usize) -> Vec<usize> {
    let mut order = order.to_vec();
    let (Some(f), Some(t)) = (
        order.iter().position(|&w| w == from),
        order.iter().position(|&w| w == to),
    ) else {
        return order;
    };
    let ws = order.remove(f);
    order.insert(t, ws);
    order
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ws(name: &str, folder: bool, open: bool, rows: usize, last_used: u64) -> Summary {
        Summary {
            name: name.into(),
            folder,
            open,
            rows,
            last_used,
        }
    }

    #[test]
    fn folders_sort_and_loose_files_stay_last() {
        let list = [
            ws("", false, true, 1, 9),
            ws("zeta", true, true, 0, 1),
            ws("Alpha", true, true, 0, 3),
            ws("mid", true, true, 0, 2),
        ];
        assert_eq!(order(&list, Sort::Manual), [1, 2, 3, 0]);
        assert_eq!(order(&list, Sort::Name), [2, 3, 1, 0]);
        assert_eq!(order(&list, Sort::Recent), [2, 3, 1, 0]);
    }

    #[test]
    fn open_workspaces_list_their_rows_and_collapsed_ones_only_a_header() {
        let list = [
            ws("a", true, true, 2, 0),
            ws("b", true, false, 5, 0),
            ws("c", true, true, 0, 0),
            ws("", false, true, 0, 0),
        ];
        let items = layout(&list, &order(&list, Sort::Manual));
        assert_eq!(
            items,
            [
                Item::Header(0),
                Item::Entry { ws: 0, row: 0 },
                Item::Entry { ws: 0, row: 1 },
                Item::Header(1),
                Item::Header(2),
                Item::Empty(2),
            ],
            "empty loose files are hidden"
        );
        assert_eq!(items[5].ws(), 2);
    }

    #[test]
    fn dragging_takes_the_target_place() {
        assert_eq!(reorder(&[0, 1, 2, 3], 0, 2), [1, 2, 0, 3]);
        assert_eq!(reorder(&[0, 1, 2, 3], 3, 1), [0, 3, 1, 2]);
        assert_eq!(reorder(&[2, 0, 1], 1, 2), [1, 2, 0]);
        assert_eq!(reorder(&[0, 1], 5, 0), [0, 1], "unknown is a no-op");
    }
}
