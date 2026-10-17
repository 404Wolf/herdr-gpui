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
