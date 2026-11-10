use clap::Parser;
use mux::pane::PaneId;
use wezterm_client::client::Client;

#[derive(Debug, Parser, Clone)]
pub struct SwapPanes {
    /// One of the panes. The default is the current pane, from the
    /// environment variable WSB_WEZTERM_PANE.
    #[arg(long)]
    pane_id: Option<PaneId>,

    /// The other pane.
    #[arg(long)]
    with: PaneId,
}

impl SwapPanes {
    pub async fn run(self, client: Client) -> anyhow::Result<()> {
        let pane_id = client.resolve_pane_id(self.pane_id).await?;
        client
            .swap_panes(codec::SwapPanes {
                pane_id,
                other_pane_id: self.with,
            })
            .await?;
        Ok(())
    }
}
