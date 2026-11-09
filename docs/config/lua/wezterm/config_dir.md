---
title: wezterm.config_dir
tags:
 - filesystem
---

# `wezterm.config_dir`

This constant is set to the path to the directory in which your `wsb-wezterm.lua`
configuration file was found.

```lua
local wezterm = require 'wsb-wezterm'
wezterm.log_error('Config Dir ' .. wezterm.config_dir)
```


