use crate::cli::layout::{resolve_tab, tab_tree};
use anyhow::anyhow;
use clap::Parser;
use mux::pane::PaneId;
use mux::tab::TabId;
use std::io::Write;
use wezterm_client::client::Client;
use wezterm_term::TerminalSize;

/// The exit status of a resize the multiplexer refused because a desktop
/// window sizes the tab.
pub const SIZED_BY_WINDOW: i32 = 3;

#[derive(Debug, Parser, Clone)]
pub struct ResizeTab {
    /// The tab to resize. The default is the tab of `--pane-id`.
    #[arg(long, conflicts_with = "pane_id")]
    tab_id: Option<TabId>,

    /// Resize the tab that holds this pane. The default is the current pane,
    /// from the environment variable WSB_WEZTERM_PANE.
    #[arg(long)]
    pane_id: Option<PaneId>,

    /// The columns the tab gets.
    #[arg(long)]
    cols: usize,

    /// The rows the tab gets.
    #[arg(long)]
    rows: usize,
}

impl ResizeTab {
    pub async fn run(self, client: Client) -> anyhow::Result<()> {
        let tab_id = resolve_tab(&client, self.tab_id, self.pane_id).await?;
        let panes = client.list_panes().await?;
        let dpi = tab_tree(&panes, tab_id)
            .and_then(|tree| tree.root_size())
            .ok_or_else(|| anyhow!("there is no tab {tab_id}"))?
            .dpi;
        let answer = client
            .resize_tab(codec::ResizeTab {
                tab_id,
                size: TerminalSize {
                    cols: self.cols,
                    rows: self.rows,
                    pixel_width: 0,
                    pixel_height: 0,
                    dpi,
                },
            })
            .await?;
        let mut out = std::io::stdout();
        writeln!(
            out,
            "{}",
            serde_json::json!({ "sized_by_window": answer.sized_by_window })
        )?;
        out.flush()?;
        if answer.sized_by_window.is_some() {
            std::process::exit(SIZED_BY_WINDOW);
        }
        Ok(())
    }
}
