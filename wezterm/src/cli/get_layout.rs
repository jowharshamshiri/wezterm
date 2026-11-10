use crate::cli::layout::{read_layout, resolve_tab};
use clap::Parser;
use mux::pane::PaneId;
use mux::tab::TabId;
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

impl GetLayout {
    pub async fn run(self, client: Client) -> anyhow::Result<()> {
        let tab_id = resolve_tab(&client, self.tab_id, self.pane_id).await?;
        let layout = read_layout(&client, tab_id).await?;
        println!("{}", serde_json::to_string_pretty(&layout)?);
        Ok(())
    }
}
