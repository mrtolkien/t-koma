# TUI upgrade

Let's make a better TUI, the current one is functional but ugly.

You use ratatui, you are powerful and able to do better!

Take inspiration from the following RataTUI projects:

- bottom
- joshuto

Other TUIs I like are:

- k9s
- Lazygit
- Lazyvim
- btop
- nvtop
- zellij
- yazi

I'm not 100% set on the layout, but I imagine something like:

- Critical information at the top (server IP, messages in last 15/5/1 minutes,
  number of of active operators, ghosts, # of pending operators as warning ...)
- Vertical tabs on the left with a big content tab at its right
- Active shortcuts at the bottom

- Rough tabs:
  - Gateway management: view and interactively edit config, see gateway status,
    start/stop/restart it, ...
  - Log tailing w/ pretty printing and colors -> you should also review all
    logging so far and use proper structured logging
  - Operators management (also includes interfaces management)
  - Ghosts management (includes speaking with a ghost)
  - Any other tab you deem useful

The vibe should be cyberpunky, harsh.

Most interactions should allow for both arrows and HJKL for navigation,
backspace to go back, ? for shortcuts... Use Vim-inspired shortcuts

Try to limit user written input to a minimum: show clear options
