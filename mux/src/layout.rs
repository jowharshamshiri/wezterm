//! Arranging a tab's panes as a whole: swapping any two panes, and giving a
//! tab a layout described as a tree of splits.

use crate::domain::DomainState;
use crate::pane::{CachePolicy, Pane, PaneId};
use crate::tab::{SplitDirection, SplitDirectionAndSize, SplitSize, TabId, Tree};
use crate::Mux;
use anyhow::{anyhow, bail};
use config::keyassignment::SpawnTabDomain;
use portable_pty::CommandBuilder;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use wezterm_term::TerminalSize;

/// A tab's layout as a client asks for it: a tree of splits whose leaves
/// are panes. This type is used directly by the codec; take care to bump
/// the codec version if you change it.
#[derive(Deserialize, Serialize, PartialEq, Debug, Clone)]
pub enum LayoutNode {
    Split {
        direction: SplitDirection,
        /// The size of the first part along the split's axis: cells, or a
        /// percentage of the cells both parts share (all but the divider's).
        first_size: SplitSize,
        first: Box<LayoutNode>,
        second: Box<LayoutNode>,
    },
    Pane(LayoutPane),
}

/// A pane of a layout.
#[derive(Deserialize, Serialize, PartialEq, Debug, Clone)]
pub struct LayoutPane {
    pub source: LayoutPaneSource,
    /// The pane gets the focus.
    pub active: bool,
    /// The pane is zoomed, and so gets the focus.
    pub zoomed: bool,
}

#[derive(Deserialize, Serialize, PartialEq, Debug, Clone)]
pub enum LayoutPaneSource {
    /// A pane already in the mux, in this tab or in another, which moves
    /// here.
    Existing(PaneId),
    /// A new pane, spawned in the domain of the tab's focused pane.
    Spawn {
        command: Option<CommandBuilder>,
        /// Its working directory; without it, the focused pane's.
        command_dir: Option<String>,
    },
}

impl LayoutNode {
    /// The fewest (columns, rows) the layout fits in.
    fn min_size(&self) -> (usize, usize) {
        match self {
            Self::Pane(_) => (1, 1),
            Self::Split {
                direction,
                first,
                second,
                ..
            } => {
                let (first_cols, first_rows) = first.min_size();
                let (second_cols, second_rows) = second.min_size();
                match direction {
                    SplitDirection::Horizontal => {
                        (first_cols + 1 + second_cols, first_rows.max(second_rows))
                    }
                    SplitDirection::Vertical => {
                        (first_cols.max(second_cols), first_rows + 1 + second_rows)
                    }
                }
            }
        }
    }

    /// The layout's panes, in order from top left.
    fn panes(&self) -> Vec<&LayoutPane> {
        match self {
            Self::Pane(pane) => vec![pane],
            Self::Split { first, second, .. } => {
                let mut panes = first.panes();
                panes.extend(second.panes());
                panes
            }
        }
    }
}

/// Where a split of a layout is, for errors: `root`, `root.first.second`.
fn place(at: &str, part: &str) -> String {
    format!("{at}.{part}")
}

fn extent(direction: SplitDirection, size: &TerminalSize) -> usize {
    match direction {
        SplitDirection::Horizontal => size.cols,
        SplitDirection::Vertical => size.rows,
    }
}

fn axis_name(direction: SplitDirection) -> &'static str {
    match direction {
        SplitDirection::Horizontal => "columns",
        SplitDirection::Vertical => "rows",
    }
}

/// A size of `cols` by `rows` cells of size `cell`.
fn cells(cols: usize, rows: usize, cell: &TerminalSize) -> TerminalSize {
    TerminalSize {
        cols,
        rows,
        pixel_width: cols * cell.pixel_width,
        pixel_height: rows * cell.pixel_height,
        dpi: cell.dpi,
    }
}

/// The sizes of the two parts of a split of `size`, its first part `first`
/// cells along the split's axis.
fn split_parts(
    direction: SplitDirection,
    size: &TerminalSize,
    first: usize,
    cell: &TerminalSize,
) -> SplitDirectionAndSize {
    let second = extent(direction, size) - 1 - first;
    let (first, second) = match direction {
        SplitDirection::Horizontal => (
            cells(first, size.rows, cell),
            cells(second, size.rows, cell),
        ),
        SplitDirection::Vertical => (
            cells(size.cols, first, cell),
            cells(size.cols, second, cell),
        ),
    };
    SplitDirectionAndSize {
        direction,
        first,
        second,
    }
}

/// Checks that `node`, at `at`, fits in `size`, and returns each pane's
/// size, in order from top left.
fn plan(
    node: &LayoutNode,
    size: &TerminalSize,
    cell: &TerminalSize,
    at: &str,
) -> anyhow::Result<Vec<TerminalSize>> {
    match node {
        LayoutNode::Pane(_) => Ok(vec![*size]),
        LayoutNode::Split {
            direction,
            first_size,
            first,
            second,
        } => {
            let axis = axis_name(*direction);
            let extent = extent(*direction, size);
            let (min_cols, min_rows) = node.min_size();
            let min = extent_of(*direction, min_cols, min_rows);
            if extent < min {
                bail!("{at} needs at least {min} {axis}, and has {extent}");
            }
            let (first_min_cols, first_min_rows) = first.min_size();
            let (second_min_cols, second_min_rows) = second.min_size();
            let lowest = extent_of(*direction, first_min_cols, first_min_rows);
            let highest = extent - 1 - extent_of(*direction, second_min_cols, second_min_rows);
            let cells = first_cells(*first_size, extent);
            if cells < lowest || cells > highest {
                let asked = match first_size {
                    SplitSize::Cells(n) => format!("{n}"),
                    SplitSize::Percent(p) => format!("{cells} ({p}%)"),
                };
                bail!(
                    "{at} divides {extent} {axis}: its first part can have \
                     {lowest} to {highest}, not {asked}"
                );
            }
            let parts = split_parts(*direction, size, cells, cell);
            let mut sizes = plan(first, &parts.first, cell, &place(at, "first"))?;
            sizes.extend(plan(second, &parts.second, cell, &place(at, "second"))?);
            Ok(sizes)
        }
    }
}

fn extent_of(direction: SplitDirection, cols: usize, rows: usize) -> usize {
    match direction {
        SplitDirection::Horizontal => cols,
        SplitDirection::Vertical => rows,
    }
}

/// The cells the first part of a split of `extent` cells gets.
fn first_cells(size: SplitSize, extent: usize) -> usize {
    match size {
        SplitSize::Cells(n) => n,
        SplitSize::Percent(p) => extent.saturating_sub(1) * p as usize / 100,
    }
}

/// The tree of `node`, planned to fit `size` by `plan`, with `panes` as its
/// panes in order from top left.
fn build(
    node: LayoutNode,
    size: &TerminalSize,
    cell: &TerminalSize,
    panes: &mut impl Iterator<Item = Arc<dyn Pane>>,
) -> Tree {
    match node {
        LayoutNode::Pane(_) => Tree::Leaf(panes.next().expect("a pane for each of the layout's")),
        LayoutNode::Split {
            direction,
            first_size,
            first,
            second,
        } => {
            let parts = split_parts(
                direction,
                size,
                first_cells(first_size, extent(direction, size)),
                cell,
            );
            Tree::Node {
                left: Box::new(build(*first, &parts.first, cell, panes)),
                right: Box::new(build(*second, &parts.second, cell, panes)),
                data: Some(parts),
            }
        }
    }
}

impl Mux {
    /// Fails, naming the pane, if this mux only mirrors the layout of the
    /// pane's tab.
    fn check_arranged_here(&self, pane: &Arc<dyn Pane>) -> anyhow::Result<()> {
        let domain = self
            .get_domain(pane.domain_id())
            .ok_or_else(|| anyhow!("the domain of pane {} is gone", pane.pane_id()))?;
        if domain.mirrors_remote_layout() {
            bail!(
                "pane {} mirrors a pane of domain {:?}, which arranges its panes itself; \
                 ask that domain's multiplexer",
                pane.pane_id(),
                domain.domain_name()
            );
        }
        Ok(())
    }

    /// The pane `pane_id` and the tab it is in.
    fn pane_and_tab(&self, pane_id: PaneId) -> anyhow::Result<(Arc<dyn Pane>, TabId)> {
        let pane = self
            .get_pane(pane_id)
            .ok_or_else(|| anyhow!("there is no pane {pane_id}"))?;
        let (_, _, tab_id) = self
            .resolve_pane_id(pane_id)
            .ok_or_else(|| anyhow!("pane {pane_id} is in no tab"))?;
        Ok((pane, tab_id))
    }

    /// Swaps two panes, in one tab or in two, each taking the other's place
    /// and size. In one tab the focus stays with the pane that had it; across
    /// two, each tab's focused place keeps the focus.
    pub fn swap_panes(&self, a: PaneId, b: PaneId) -> anyhow::Result<()> {
        let (pane_a, tab_a) = self.pane_and_tab(a)?;
        let (pane_b, tab_b) = self.pane_and_tab(b)?;
        self.check_arranged_here(&pane_a)?;
        self.check_arranged_here(&pane_b)?;
        let tab = |id| {
            self.get_tab(id)
                .ok_or_else(|| anyhow!("there is no tab {id}"))
        };
        if tab_a == tab_b {
            return tab(tab_a)?.swap_panes(a, b);
        }
        tab(tab_a)?.replace_pane(a, pane_b)?;
        tab(tab_b)?.replace_pane(b, pane_a)?;
        Ok(())
    }

    /// Gives the tab `tab_id` the layout `root`, sized to the tab. Every pane
    /// already in the tab must be in the layout; panes of other tabs move in,
    /// and the tabs they leave empty close. Fails, changing nothing, when the
    /// layout names a pane twice, a pane that does not exist, or has sizes
    /// that do not fit.
    pub async fn set_tab_layout(&self, tab_id: TabId, root: LayoutNode) -> anyhow::Result<()> {
        let tab = self
            .get_tab(tab_id)
            .ok_or_else(|| anyhow!("there is no tab {tab_id}"))?;

        let leaves = root.panes();
        let mut named: Vec<PaneId> = vec![];
        for leaf in &leaves {
            if let LayoutPaneSource::Existing(pane_id) = leaf.source {
                if named.contains(&pane_id) {
                    bail!("the layout names pane {pane_id} more than once");
                }
                named.push(pane_id);
            }
        }
        let mut moving = vec![];
        for &pane_id in &named {
            let (pane, from) = self.pane_and_tab(pane_id)?;
            self.check_arranged_here(&pane)?;
            if from != tab_id {
                moving.push((pane_id, from));
            }
        }
        let left_out: Vec<String> = tab
            .iter_panes_ignoring_zoom()
            .iter()
            .map(|p| p.pane.pane_id())
            .filter(|id| !named.contains(id))
            .map(|id| id.to_string())
            .collect();
        if !left_out.is_empty() {
            bail!(
                "the layout leaves out pane {} of tab {tab_id}; place every pane of the tab, \
                 or close it first",
                left_out.join(", pane ")
            );
        }
        let focus = |wanted: fn(&LayoutPane) -> bool, what: &str| {
            let at: Vec<usize> = (0..leaves.len()).filter(|&i| wanted(leaves[i])).collect();
            match at.as_slice() {
                [] => Ok(None),
                [one] => Ok(Some(*one)),
                _ => Err(anyhow!(
                    "the layout makes {} panes {what}; one at most",
                    at.len()
                )),
            }
        };
        let active = focus(|pane| pane.active, "active")?;
        let zoomed = focus(|pane| pane.zoomed, "zoomed")?;
        if let (Some(active), Some(zoomed)) = (active, zoomed) {
            if active != zoomed {
                bail!("the zoomed pane is the active one; the layout makes another pane active");
            }
        }

        let size = tab.get_size();
        let cell = TerminalSize {
            cols: 1,
            rows: 1,
            pixel_width: size.pixel_width / size.cols.max(1),
            pixel_height: size.pixel_height / size.rows.max(1),
            dpi: size.dpi,
        };
        let sizes = plan(&root, &size, &cell, "root")?;

        let current = tab
            .get_active_pane()
            .ok_or_else(|| anyhow!("tab {tab_id} has no pane"))?;
        let mut spawned: Vec<Arc<dyn Pane>> = vec![];
        let spawns: Vec<_> = leaves
            .iter()
            .zip(&sizes)
            .filter_map(|(leaf, size)| match &leaf.source {
                LayoutPaneSource::Spawn {
                    command,
                    command_dir,
                } => Some((command.clone(), command_dir.clone(), *size)),
                LayoutPaneSource::Existing(_) => None,
            })
            .collect();
        if !spawns.is_empty() {
            let domain = self.resolve_spawn_tab_domain(
                Some(current.pane_id()),
                &SpawnTabDomain::CurrentPaneDomain,
            )?;
            if domain.mirrors_remote_layout() {
                bail!(
                    "tab {tab_id} mirrors a tab of domain {:?}, which arranges its panes itself; \
                     ask that domain's multiplexer",
                    domain.domain_name()
                );
            }
            if domain.state() == DomainState::Detached {
                domain.attach(self.window_containing_tab(tab_id)).await?;
            }
            for (command, command_dir, size) in spawns {
                let command_dir = self.resolve_cwd(
                    command_dir,
                    Some(Arc::clone(&current)),
                    domain.domain_id(),
                    CachePolicy::FetchImmediate,
                );
                match domain.spawn_pane(size, command, command_dir).await {
                    Ok(pane) => spawned.push(pane),
                    Err(err) => {
                        for pane in spawned {
                            self.remove_pane(pane.pane_id());
                        }
                        return Err(err.context("spawning a pane of the layout"));
                    }
                }
            }
        }

        // Spawning awaited: the panes the layout names are looked up again.
        let mut existing = vec![];
        for &pane_id in &named {
            match self.get_pane(pane_id) {
                Some(pane) => existing.push(pane),
                None => {
                    for pane in spawned {
                        self.remove_pane(pane.pane_id());
                    }
                    bail!("pane {pane_id} closed while the layout's panes were spawned");
                }
            }
        }

        for (pane_id, from) in moving {
            if let Some(from) = self.get_tab(from) {
                from.remove_pane(pane_id);
                if from.is_dead() {
                    self.remove_tab(from.tab_id());
                }
            }
        }

        let mut existing = existing.into_iter();
        let mut spawned_iter = spawned.into_iter();
        let panes: Vec<Arc<dyn Pane>> = leaves
            .iter()
            .map(|leaf| match leaf.source {
                LayoutPaneSource::Existing(_) => existing.next(),
                LayoutPaneSource::Spawn { .. } => spawned_iter.next(),
            })
            .map(|pane| pane.expect("a pane for each of the layout's"))
            .collect();
        let id_at = |index: Option<usize>| index.map(|i| panes[i].pane_id());
        let (active, zoomed) = (id_at(active), id_at(zoomed));
        let tree = build(root, &size, &cell, &mut panes.clone().into_iter());
        tab.set_tree(tree, active, zoomed);
        Ok(())
    }
}

#[cfg(test)]
mod test {
    use super::*;

    fn pane(id: PaneId) -> LayoutNode {
        LayoutNode::Pane(LayoutPane {
            source: LayoutPaneSource::Existing(id),
            active: false,
            zoomed: false,
        })
    }

    fn split(
        direction: SplitDirection,
        first_size: SplitSize,
        a: LayoutNode,
        b: LayoutNode,
    ) -> LayoutNode {
        LayoutNode::Split {
            direction,
            first_size,
            first: Box::new(a),
            second: Box::new(b),
        }
    }

    fn size(cols: usize, rows: usize) -> TerminalSize {
        TerminalSize {
            cols,
            rows,
            pixel_width: cols * 10,
            pixel_height: rows * 20,
            dpi: 96,
        }
    }

    fn cell() -> TerminalSize {
        size(1, 1)
    }

    #[test]
    fn a_plan_gives_each_pane_its_part_of_the_tab() {
        // 80x24: pane 1 (30 columns) | [pane 2 (top 10 rows) / pane 3]
        let layout = split(
            SplitDirection::Horizontal,
            SplitSize::Cells(30),
            pane(1),
            split(
                SplitDirection::Vertical,
                SplitSize::Cells(10),
                pane(2),
                pane(3),
            ),
        );
        let sizes = plan(&layout, &size(80, 24), &cell(), "root").unwrap();
        assert_eq!(sizes, vec![size(30, 24), size(49, 10), size(49, 13)]);
    }

    #[test]
    fn a_percentage_is_of_the_cells_both_parts_share() {
        let layout = split(
            SplitDirection::Horizontal,
            SplitSize::Percent(25),
            pane(1),
            pane(2),
        );
        let sizes = plan(&layout, &size(81, 24), &cell(), "root").unwrap();
        assert_eq!(sizes, vec![size(20, 24), size(60, 24)]);
    }

    #[test]
    fn a_part_too_small_for_its_panes_is_refused_by_where_it_is() {
        let layout = split(
            SplitDirection::Horizontal,
            SplitSize::Cells(70),
            pane(1),
            split(
                SplitDirection::Horizontal,
                SplitSize::Cells(8),
                pane(2),
                pane(3),
            ),
        );
        let err = plan(&layout, &size(80, 24), &cell(), "root").unwrap_err();
        assert_eq!(
            err.to_string(),
            "root.second divides 9 columns: its first part can have 1 to 7, not 8"
        );
        let layout = split(
            SplitDirection::Horizontal,
            SplitSize::Cells(78),
            pane(1),
            split(
                SplitDirection::Horizontal,
                SplitSize::Cells(1),
                pane(2),
                pane(3),
            ),
        );
        let err = plan(&layout, &size(80, 24), &cell(), "root").unwrap_err();
        assert_eq!(
            err.to_string(),
            "root divides 80 columns: its first part can have 1 to 76, not 78"
        );
        let layout = split(
            SplitDirection::Vertical,
            SplitSize::Percent(50),
            pane(1),
            pane(2),
        );
        let err = plan(&layout, &size(80, 2), &cell(), "root").unwrap_err();
        assert_eq!(err.to_string(), "root needs at least 3 rows, and has 2");
    }
}
