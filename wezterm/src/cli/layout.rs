//! A tab's layout as `wsb-wezterm cli` reads and writes it: JSON for the
//! tree of splits the multiplexer keeps (`mux::tab::PaneNode`), and for the
//! layout `set-layout` asks for (`mux::layout::LayoutNode`).

use anyhow::{anyhow, bail};
use codec::ListPanesResponse;
use mux::layout::{LayoutNode, LayoutPane, LayoutPaneSource};
use mux::pane::PaneId;
use mux::tab::{PaneEntry, PaneNode, SplitDirection, SplitSize, TabId};
use mux::window::WindowId;
use portable_pty::CommandBuilder;
use serde::{Deserialize, Serialize};
use std::ffi::OsString;
use wezterm_client::client::Client;

/// How a split divides: `horizontal` puts its parts side by side, the first
/// on the left; `vertical` stacks them, the first on top.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Direction {
    Horizontal,
    Vertical,
}

impl From<SplitDirection> for Direction {
    fn from(direction: SplitDirection) -> Self {
        match direction {
            SplitDirection::Horizontal => Self::Horizontal,
            SplitDirection::Vertical => Self::Vertical,
        }
    }
}

impl From<Direction> for SplitDirection {
    fn from(direction: Direction) -> Self {
        match direction {
            Direction::Horizontal => Self::Horizontal,
            Direction::Vertical => Self::Vertical,
        }
    }
}

/// A tab's layout: where each pane is and how big, in cells from the tab's
/// top left. While a pane is zoomed the tree keeps the places the panes have
/// when it is not.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct TabLayout {
    pub tab_id: TabId,
    pub window_id: WindowId,
    pub cols: usize,
    pub rows: usize,
    /// The focused pane.
    pub active: Option<PaneId>,
    /// The zoomed pane, which then fills the tab.
    pub zoomed: Option<PaneId>,
    pub root: Node,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Node {
    Pane(PanePlace),
    Split(SplitPlace),
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct PanePlace {
    pub id: PaneId,
    pub left: usize,
    pub top: usize,
    pub cols: usize,
    pub rows: usize,
    pub title: String,
    /// The working directory, as a URL, where the pane can tell.
    pub cwd: Option<String>,
    /// The program in the foreground, as the path of its executable, where
    /// the pane can tell.
    pub process: Option<String>,
    /// The name of the pane's terminal device, where it has one.
    pub tty: Option<String>,
}

/// A split: the 1-cell divider comes right after (or below) `first`.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SplitPlace {
    pub direction: Direction,
    pub left: usize,
    pub top: usize,
    pub cols: usize,
    pub rows: usize,
    pub first: Box<Node>,
    pub second: Box<Node>,
}

impl TabLayout {
    /// The layout of the tab whose tree is `root`, or None for an empty one.
    pub fn from_tree(root: &PaneNode) -> Option<Self> {
        let (window_id, tab_id) = root.window_and_tab_ids()?;
        let (cols, rows) = match root {
            PaneNode::Empty => return None,
            PaneNode::Leaf(entry) => (entry.size.cols, entry.size.rows),
            PaneNode::Split { node, .. } => (node.width(), node.height()),
        };
        let mut active = None;
        let mut zoomed = None;
        let root = place(root, 0, 0, cols, rows, &mut |entry| {
            if entry.is_active_pane {
                active = Some(entry.pane_id);
            }
            if entry.is_zoomed_pane {
                zoomed = Some(entry.pane_id);
            }
        })?;
        Some(Self {
            tab_id,
            window_id,
            cols,
            rows,
            active,
            zoomed,
            root,
        })
    }

    /// The ids of the layout's panes, in order from top left.
    pub fn pane_ids(&self) -> Vec<PaneId> {
        fn walk(node: &Node, ids: &mut Vec<PaneId>) {
            match node {
                Node::Pane(pane) => ids.push(pane.id),
                Node::Split(split) => {
                    walk(&split.first, ids);
                    walk(&split.second, ids);
                }
            }
        }
        let mut ids = vec![];
        walk(&self.root, &mut ids);
        ids
    }
}

fn place(
    node: &PaneNode,
    left: usize,
    top: usize,
    cols: usize,
    rows: usize,
    seen: &mut impl FnMut(&PaneEntry),
) -> Option<Node> {
    match node {
        PaneNode::Empty => None,
        PaneNode::Leaf(entry) => {
            seen(entry);
            Some(Node::Pane(PanePlace {
                id: entry.pane_id,
                left,
                top,
                cols,
                rows,
                title: entry.title.clone(),
                cwd: entry.working_dir.as_ref().map(|url| url.url.to_string()),
                process: entry.foreground_process_name.clone(),
                tty: entry.tty_name.clone(),
            }))
        }
        PaneNode::Split {
            left: first,
            right: second,
            node: split,
        } => {
            let (second_left, second_top) = match split.direction {
                SplitDirection::Horizontal => (left + split.first.cols + 1, top),
                SplitDirection::Vertical => (left, top + split.first.rows + 1),
            };
            Some(Node::Split(SplitPlace {
                direction: split.direction.into(),
                left,
                top,
                cols,
                rows,
                first: Box::new(place(
                    first,
                    left,
                    top,
                    split.first.cols,
                    split.first.rows,
                    seen,
                )?),
                second: Box::new(place(
                    second,
                    second_left,
                    second_top,
                    split.second.cols,
                    split.second.rows,
                    seen,
                )?),
            }))
        }
    }
}

/// The tree of the tab `tab_id` in a listing of the multiplexer's panes.
pub fn tab_tree(panes: &ListPanesResponse, tab_id: TabId) -> Option<&PaneNode> {
    panes
        .tabs
        .iter()
        .find(|tree| tree.window_and_tab_ids().map(|(_, tab)| tab) == Some(tab_id))
}

/// The tab that holds the pane `pane_id` in a listing of the multiplexer's
/// panes.
pub fn tab_of_pane(panes: &ListPanesResponse, pane_id: PaneId) -> Option<TabId> {
    panes
        .tabs
        .iter()
        .filter_map(TabLayout::from_tree)
        .find(|layout| layout.pane_ids().contains(&pane_id))
        .map(|layout| layout.tab_id)
}

/// The tab a command acts on: `tab_id`, else the tab of `pane_id`, else the
/// tab of the current pane (`WSB_WEZTERM_PANE`).
pub async fn resolve_tab(
    client: &Client,
    tab_id: Option<TabId>,
    pane_id: Option<PaneId>,
) -> anyhow::Result<TabId> {
    if let Some(tab_id) = tab_id {
        return Ok(tab_id);
    }
    let pane_id = client.resolve_pane_id(pane_id).await?;
    let panes = client.list_panes().await?;
    tab_of_pane(&panes, pane_id).ok_or_else(|| anyhow!("there is no pane {pane_id}"))
}

/// The layout of the tab `tab_id`, as the multiplexer has it now.
pub async fn read_layout(client: &Client, tab_id: TabId) -> anyhow::Result<TabLayout> {
    let panes = client.list_panes().await?;
    tab_tree(&panes, tab_id)
        .and_then(TabLayout::from_tree)
        .ok_or_else(|| anyhow!("there is no tab {tab_id}"))
}

/// Whether a desktop window sizes the tab `tab_id`, as the multiplexer says
/// it in a listing of its panes: the rule and each window that does, or None
/// where no window does.
pub fn tab_sized_by_window(panes: &ListPanesResponse, tab_id: TabId) -> Option<String> {
    panes
        .tabs
        .iter()
        .position(|tree| tree.window_and_tab_ids().map(|(_, tab)| tab) == Some(tab_id))
        .and_then(|index| panes.tab_sized_by_window.get(index).cloned().flatten())
}

/// A layout as `set-layout` takes it.
#[derive(Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct LayoutRequest {
    pub root: RequestNode,
}

#[derive(Debug, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum RequestNode {
    Pane(RequestPane),
    Split(RequestSplit),
}

/// A pane of the layout: `id`, a pane already in the multiplexer, or
/// `spawn`, a new one.
#[derive(Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct RequestPane {
    pub id: Option<PaneId>,
    pub spawn: Option<RequestSpawn>,
    #[serde(default)]
    pub active: bool,
    #[serde(default)]
    pub zoomed: bool,
}

#[derive(Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct RequestSpawn {
    /// The program and its arguments; empty runs the default program.
    #[serde(default)]
    pub args: Vec<String>,
    /// Its working directory, relative to the command's own; without it,
    /// the focused pane's.
    pub cwd: Option<String>,
}

/// A split: the size of its first part is `first_cells` cells, or
/// `first_percent` percent of the cells both parts share; without either,
/// half of them.
#[derive(Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct RequestSplit {
    pub direction: Direction,
    pub first_cells: Option<usize>,
    pub first_percent: Option<u8>,
    pub first: Box<RequestNode>,
    pub second: Box<RequestNode>,
}

impl RequestNode {
    /// The layout the multiplexer takes; `at` names where this node is, for
    /// errors.
    pub fn to_layout(self, at: &str) -> anyhow::Result<LayoutNode> {
        match self {
            Self::Pane(RequestPane {
                id,
                spawn,
                active,
                zoomed,
            }) => {
                let source = match (id, spawn) {
                    (Some(id), None) => LayoutPaneSource::Existing(id),
                    (None, Some(RequestSpawn { args, cwd })) => LayoutPaneSource::Spawn {
                        command: (!args.is_empty()).then(|| {
                            CommandBuilder::from_argv(
                                args.into_iter().map(OsString::from).collect(),
                            )
                        }),
                        command_dir: crate::cli::resolve_relative_cwd(cwd.map(OsString::from))?,
                    },
                    (Some(_), Some(_)) => bail!("the pane at {at} has both an id and a spawn"),
                    (None, None) => bail!("the pane at {at} has neither an id nor a spawn"),
                };
                Ok(LayoutNode::Pane(LayoutPane {
                    source,
                    active,
                    zoomed,
                }))
            }
            Self::Split(RequestSplit {
                direction,
                first_cells,
                first_percent,
                first,
                second,
            }) => {
                let first_size = match (first_cells, first_percent) {
                    (Some(cells), None) => SplitSize::Cells(cells),
                    (None, Some(percent)) if percent <= 100 => SplitSize::Percent(percent),
                    (None, Some(percent)) => {
                        bail!("the split at {at} asks for {percent} percent; at most 100")
                    }
                    (None, None) => SplitSize::Percent(50),
                    (Some(_), Some(_)) => {
                        bail!("the split at {at} has both first_cells and first_percent")
                    }
                };
                Ok(LayoutNode::Split {
                    direction: direction.into(),
                    first_size,
                    first: Box::new(first.to_layout(&format!("{at}.first"))?),
                    second: Box::new(second.to_layout(&format!("{at}.second"))?),
                })
            }
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn a_request_names_what_it_lacks_and_what_it_has_twice() {
        let request: LayoutRequest = serde_json::from_str(
            r#"{"root":{"split":{"direction":"vertical","first_cells":3,
                "first":{"pane":{"id":1,"active":true}},
                "second":{"pane":{}}}}}"#,
        )
        .unwrap();
        assert_eq!(
            request.root.to_layout("root").unwrap_err().to_string(),
            "the pane at root.second has neither an id nor a spawn"
        );
        let request: LayoutRequest =
            serde_json::from_str(r#"{"root":{"pane":{"id":1,"spawn":{}}}}"#).unwrap();
        assert_eq!(
            request.root.to_layout("root").unwrap_err().to_string(),
            "the pane at root has both an id and a spawn"
        );
    }

    #[test]
    fn a_request_refuses_fields_it_does_not_take() {
        // A layout read back carries places and titles, which a request
        // cannot set.
        assert!(
            serde_json::from_str::<LayoutRequest>(r#"{"root":{"pane":{"id":1,"cols":80}}}"#)
                .is_err()
        );
        assert!(serde_json::from_str::<LayoutRequest>(
            r#"{"root":{"split":{"direction":"diagonal","first":{"pane":{"id":1}},"second":{"pane":{"id":2}}}}}"#
        )
        .is_err());
    }

    #[test]
    fn a_split_without_a_size_is_halved() {
        let request: LayoutRequest = serde_json::from_str(
            r#"{"root":{"split":{"direction":"horizontal",
                "first":{"pane":{"id":1}},"second":{"pane":{"spawn":{"args":["top"]}}}}}}"#,
        )
        .unwrap();
        match request.root.to_layout("root").unwrap() {
            LayoutNode::Split {
                direction,
                first_size,
                ..
            } => {
                assert_eq!(direction, SplitDirection::Horizontal);
                assert_eq!(first_size, SplitSize::Percent(50));
            }
            other => panic!("{:?}", other),
        }
    }
}
