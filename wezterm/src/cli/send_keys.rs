use crate::cli::relay::key_input;
use anyhow::Context;
use clap::Parser;
use codec::{InputSerial, SendKeyDown};
use config::ConfigHandle;
use mux::pane::PaneId;
use termwiz::input::KeyEvent;
use wezterm_client::client::Client;

#[derive(Debug, Parser, Clone)]
pub struct SendKeys {
    /// The pane to send the keys to. The default is the current pane, from
    /// the environment variable WSB_WEZTERM_PANE.
    #[arg(long)]
    pane_id: Option<PaneId>,

    /// The keys, pressed in order: each a key named as in a key binding
    /// (`a`, `Enter`, `UpArrow`, `F5`), after modifiers and a `+` when it
    /// has them (`CTRL+c`, `CTRL|SHIFT+t`, `CTRL++`).
    #[arg(required = true)]
    keys: Vec<String>,
}

/// The modifiers and the key of a key as `send-keys` takes it.
fn split_key(key: &str) -> (&str, &str) {
    if let Some(mods) = key.strip_suffix("++") {
        return (mods, "+");
    }
    match key.rsplit_once('+') {
        Some((mods, name)) if !mods.is_empty() && !name.is_empty() => (mods, name),
        _ => ("", key),
    }
}

impl SendKeys {
    pub async fn run(self, client: Client, config: &ConfigHandle) -> anyhow::Result<()> {
        let pane_id = client.resolve_pane_id(self.pane_id).await?;
        // Every key is checked before any is sent.
        let keys = self
            .keys
            .iter()
            .map(|key| {
                let (mods, name) = split_key(key);
                key_input(name, mods, config.swap_backspace_and_delete)
                    .with_context(|| format!("the key {key:?}"))
            })
            .collect::<anyhow::Result<Vec<_>>>()?;
        for (key, (code, modifiers)) in self.keys.iter().zip(keys) {
            client
                .key_down(SendKeyDown {
                    pane_id,
                    event: KeyEvent {
                        key: code,
                        modifiers,
                    },
                    input_serial: InputSerial::now(),
                })
                .await
                .with_context(|| format!("sending the key {key:?}"))?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod test {
    use super::split_key;

    #[test]
    fn a_key_is_split_from_its_modifiers_at_the_last_plus() {
        assert_eq!(split_key("Enter"), ("", "Enter"));
        assert_eq!(split_key("CTRL+c"), ("CTRL", "c"));
        assert_eq!(split_key("CTRL|SHIFT+t"), ("CTRL|SHIFT", "t"));
        assert_eq!(split_key("+"), ("", "+"));
        assert_eq!(split_key("CTRL++"), ("CTRL", "+"));
    }
}
