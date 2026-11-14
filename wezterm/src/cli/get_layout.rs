use crate::cli::layout::{resolve_tab, tab_sized_by_window, tab_tree, TabLayout};
use anyhow::anyhow;
use clap::Parser;
use mux::pane::PaneId;
use mux::tab::TabId;
use serde::Serialize;
use wezterm_client::client::Client;

#[derive(Debug, Parser, Clone)]
pub struct GetLayout {
    /// The tab to read. The default is the tab of `--pane-id`.
    #[arg(long, conflicts_with = "pane_id")]
    tab_id: Option<TabId>,

    /// Read the tab that holds this pane. The default is the current pane,
    /// from the environment variable WSB_WEZTERM_PANE.
    #[arg(long)]
    pane_id: Option<PaneId>,
}

/// The tab's layout, and whether a desktop window sizes the tab.
#[derive(Serialize)]
struct Shown {
    #[serde(flatten)]
    layout: TabLayout,
    /// The rule by which desktop windows size the tab and each window that
    /// does, as the multiplexer says it; null where no window does, and the
    /// tab is sized by whoever resizes it.
    sized_by_window: Option<String>,
}

impl GetLayout {
    pub async fn run(self, client: Client) -> anyhow::Result<()> {
        let tab_id = resolve_tab(&client, self.tab_id, self.pane_id).await?;
        let panes = client.list_panes().await?;
        let layout = tab_tree(&panes, tab_id)
            .and_then(TabLayout::from_tree)
            .ok_or_else(|| anyhow!("there is no tab {tab_id}"))?;
        let shown = Shown {
            layout,
            sized_by_window: tab_sized_by_window(&panes, tab_id),
        };
        println!("{}", serde_json::to_string_pretty(&shown)?);
        Ok(())
    }
}
