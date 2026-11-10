use crate::cli::layout::{read_layout, resolve_tab, LayoutRequest};
use anyhow::Context;
use clap::Parser;
use mux::pane::PaneId;
use mux::tab::TabId;
use std::io::Read;
use wezterm_client::client::Client;

#[derive(Debug, Parser, Clone)]
pub struct SetLayout {
    /// The tab to arrange. The default is the tab of `--pane-id`.
    #[arg(long, conflicts_with = "pane_id")]
    tab_id: Option<TabId>,

    /// Arrange the tab that holds this pane. The default is the current
    /// pane, from the environment variable WSB_WEZTERM_PANE.
    #[arg(long)]
    pane_id: Option<PaneId>,
}

impl SetLayout {
    pub async fn run(self, client: Client) -> anyhow::Result<()> {
        let mut text = String::new();
        std::io::stdin()
            .read_to_string(&mut text)
            .context("reading the layout from stdin")?;
        let request: LayoutRequest = serde_json::from_str(&text)
            .context("the layout on stdin is not one set-layout takes")?;
        let root = request.root.to_layout("root")?;
        let tab_id = resolve_tab(&client, self.tab_id, self.pane_id).await?;
        client
            .set_tab_layout(codec::SetTabLayout { tab_id, root })
            .await?;
        let layout = read_layout(&client, tab_id).await?;
        println!("{}", serde_json::to_string_pretty(&layout)?);
        Ok(())
    }
}
