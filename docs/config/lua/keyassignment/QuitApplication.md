# `QuitApplication`

Terminate the wsb-wezterm application, killing all tabs.

```lua
local wezterm = require 'wsb-wezterm'

config.keys = {
  { key = 'q', mods = 'CMD', action = wezterm.action.QuitApplication },
}
```


