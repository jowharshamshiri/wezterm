use clap::{Parser, ValueEnum};
use mux::pane::PaneId;
use mux::tab::SplitDirection;
use wezterm_client::client::Client;

#[derive(Debug, Clone, Copy, ValueEnum)]
enum DividerSide {
    /// The divider to the right of the pane.
    Right,
    /// The divider below the pane.
    Down,
}

#[derive(Debug, Parser, Clone)]
pub struct ResizeSplit {
    /// The pane before the divider: the split resized is the nearest one
    /// around this pane whose first part holds it and whose divider is on
    /// the `--direction` side. The default is the current pane, from the
    /// environment variable WSB_WEZTERM_PANE.
    #[arg(long)]
    pane_id: Option<PaneId>,

    /// Which divider of the pane: the one to its right, or below it.
    #[arg(long, value_enum)]
    direction: DividerSide,

    /// The cells the split's first part (left of or above the divider) gets;
    /// the second part gets the rest.
    #[arg(long)]
    first_cells: usize,
}

impl ResizeSplit {
    pub async fn run(self, client: Client) -> anyhow::Result<()> {
        let pane_id = client.resolve_pane_id(self.pane_id).await?;
        client
            .resize_split(codec::ResizeSplit {
                pane_id,
                direction: match self.direction {
                    DividerSide::Right => SplitDirection::Horizontal,
                    DividerSide::Down => SplitDirection::Vertical,
                },
                first_cells: self.first_cells,
            })
            .await?;
        Ok(())
    }
}
