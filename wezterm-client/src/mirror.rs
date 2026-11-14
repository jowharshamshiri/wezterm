//! A client's tab that mirrors a tab of the server: its size and dividers are
//! asked of the server, as one operation on the whole tab each
//! (`mux::tab::TabMirror`).

use crate::domain::ClientInner;
use crate::pane::ClientPane;
use codec::{ResizeSplit, ResizeTab, ResizeTabResponse};
use mux::pane::Pane;
use mux::tab::{SplitDirection, TabId, TabMirror};
use parking_lot::Mutex;
use std::future::Future;
use std::sync::Arc;
use wezterm_term::TerminalSize;

/// Why the server did not size a tab as it was asked.
#[derive(Debug, Clone, thiserror::Error)]
pub enum TabNotSized {
    /// A desktop window sizes the tab: the rule and each window, as the
    /// server says them.
    #[error("{0}")]
    SizedByWindow(String),
    /// The request failed: the tab is gone, the size does not fit its
    /// layout, or the connection broke.
    #[error("{0}")]
    Failed(String),
}

/// The sizes this client asks the server to give one tab. They are sent one
/// at a time, the next only once the server has answered the last, and only
/// the most recent size waits to be sent: requests sent together could be
/// applied in any order, and an earlier size could be the one the tab is
/// left at.
pub struct TabSizes {
    state: Mutex<TabSizesState>,
}

struct TabSizesState {
    /// The size most recently asked for, until it is sent.
    wanted: Option<TerminalSize>,
    in_flight: bool,
    /// Who waits for the requests in flight to end: told how the last one
    /// ended, once none is left to send.
    waiters: Vec<smol::channel::Sender<Result<(), TabNotSized>>>,
}

impl Default for TabSizes {
    fn default() -> Self {
        Self {
            state: Mutex::new(TabSizesState {
                wanted: None,
                in_flight: false,
                waiters: vec![],
            }),
        }
    }
}

impl TabSizes {
    /// Asks the server to size its tab `remote_tab_id` `size`, after the
    /// requests already asked. The future ends once no size is left to send,
    /// with how the last request ended.
    pub fn ask(
        self: &Arc<Self>,
        client: &Arc<ClientInner>,
        remote_tab_id: TabId,
        size: TerminalSize,
    ) -> impl Future<Output = Result<(), TabNotSized>> + 'static {
        let (tx, rx) = smol::channel::bounded(1);
        {
            let mut state = self.state.lock();
            state.wanted = Some(size);
            state.waiters.push(tx);
            if !state.in_flight {
                state.in_flight = true;
                promise::spawn::spawn(Arc::clone(self).send(Arc::clone(client), remote_tab_id))
                    .detach();
            }
        }
        async move {
            rx.recv().await.unwrap_or_else(|_| {
                Err(TabNotSized::Failed(
                    "the client stopped sizing the tab".to_string(),
                ))
            })
        }
    }

    async fn send(self: Arc<Self>, client: Arc<ClientInner>, remote_tab_id: TabId) {
        let mut outcome = Ok(());
        loop {
            let size = {
                let mut state = self.state.lock();
                match state.wanted.take() {
                    Some(size) => size,
                    None => {
                        state.in_flight = false;
                        for waiter in state.waiters.drain(..) {
                            waiter.try_send(outcome.clone()).ok();
                        }
                        return;
                    }
                }
            };
            outcome = match client
                .client
                .resize_tab(ResizeTab {
                    tab_id: remote_tab_id,
                    size,
                })
                .await
            {
                Ok(ResizeTabResponse {
                    sized_by_window: None,
                }) => Ok(()),
                Ok(ResizeTabResponse {
                    sized_by_window: Some(rule),
                }) => {
                    log::debug!("tab {remote_tab_id} was not sized {size:?}: {rule}");
                    Err(TabNotSized::SizedByWindow(rule))
                }
                Err(err) => {
                    let message = format!("sizing remote tab {remote_tab_id} to {size:?}: {err:#}");
                    log::error!("{message}");
                    Err(TabNotSized::Failed(message))
                }
            };
        }
    }
}

/// A tab of this client mirroring the server's tab `remote_tab_id`.
pub struct MirroredTab {
    client: Arc<ClientInner>,
    remote_tab_id: TabId,
    sizes: Arc<TabSizes>,
}

impl MirroredTab {
    pub fn new(client: &Arc<ClientInner>, remote_tab_id: TabId, sizes: Arc<TabSizes>) -> Self {
        Self {
            client: Arc::clone(client),
            remote_tab_id,
            sizes,
        }
    }
}

impl TabMirror for MirroredTab {
    fn resize(&self, size: TerminalSize) {
        // A window sizes the tabs it shows; nobody waits for the answer, and
        // a failure is logged where it is learned.
        promise::spawn::spawn(self.sizes.ask(&self.client, self.remote_tab_id, size)).detach();
    }

    fn resize_split(&self, pane: &Arc<dyn Pane>, direction: SplitDirection, first_cells: usize) {
        let Some(pane) = pane.downcast_ref::<ClientPane>() else {
            log::error!(
                "pane {} of the mirror of remote tab {} is not the server's; \
                 its divider is not moved there",
                pane.pane_id(),
                self.remote_tab_id
            );
            return;
        };
        let client = Arc::clone(&self.client);
        let request = ResizeSplit {
            pane_id: pane.remote_pane_id,
            direction,
            first_cells,
        };
        promise::spawn::spawn(async move {
            if let Err(err) = client.client.resize_split(request).await {
                log::error!("moving a divider of the server's tab: {err:#}");
            }
        })
        .detach();
    }
}
