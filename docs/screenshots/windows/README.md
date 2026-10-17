# Windows before and after

Captured on Windows 11 Pro (10.0.26300, x64) from real windows. Each "before"
comes from a build of the `windows` branch without the change, each "after" from
the branch with it.

| Change | Before | After |
| --- | --- | --- |
| Installer shows Herdr's icon instead of Inno Setup's stock graphic | ![](installer-before.png) | ![](installer-after.png) |
| `herdr-gpui.exe` carries a name, version, and icon (Properties → Details) | ![](exe-details-before.png) | ![](exe-details-after.png) |
| Ctrl-V in a WSL pane: "Image not sent" before; text pastes after (Ctrl-Shift-V shown, Ctrl-V reaches the shell as a key) | ![](wsl-ctrl-v-before.png) | ![](wsl-ctrl-v-after.png) |
| CPU and memory of a WSL distribution in the sidebar and status bar (25% under an 8-thread load) | ![](wsl-cpu-memory-before.png) | ![](wsl-cpu-memory-after.png) |

The "after" executable reports version 0.0.0.0 / dev because it is a local
build; release builds carry the release version.

## Shell integration

Captured on the same machine against isolated Herdr sessions, with the
desktop or the window copied by PowerShell (`CopyFromScreen`, `PrintWindow`):
no clicks were made, so the jump list menu, the taskbar progress bar, and a
clicked toast banner are not shown. Before these changes the taskbar button had
no badge at all, as in the first image.

| Change | Capture |
| --- | --- |
| Taskbar badge: no agent needs attention | ![](taskbar-badge-none.png) |
| One blocked agent | ![](taskbar-badge-1.png) |
| Two | ![](taskbar-badge-2.png) |
| Ten or more | ![](taskbar-badge-9plus.png) |
| `herdr-gpui://open?path=…` for a checkout with no workspace asks first | ![](link-open-checkout.png) |
| The same link started by the Windows shell (`Start-Process`) from outside the app | ![](link-from-shell.png) |
| A link to a folder that is not a Git checkout is refused in the status bar | ![](link-not-a-checkout.png) |
| The jump list's Show Local and Show Ubuntu-22.04 tasks' commands, run as the shell runs them | ![](jump-list-show-local.png) ![](jump-list-show-wsl.png) |
| A blocked agent's toast, before and after its protocol link (read from Windows' notification history) is run as a click runs it: the pane's workspace is focused | ![](notification-click-before.png) ![](notification-click-after.png) |
