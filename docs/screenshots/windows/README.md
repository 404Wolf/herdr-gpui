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
| Title bar follows the app's dark theme under light Windows (a light theme keeps a light one: ![](title-bar-light-theme-after.png)) | ![](title-bar-theme-before.png) | ![](title-bar-theme-after.png) |
| `[backdrop] opacity = 0.6, blur = true`: composition blur under a solid title bar before; DWM acrylic over the window and title bar after | ![](backdrop-blur-before.png) | ![](backdrop-acrylic-after.png) |
| `material = "mica"` (after only: Mica tints the window with the wallpaper, not the windows behind) | | ![](backdrop-mica-after.png) |
| `accent = "system"`: the current tab takes the Windows accent (this machine's is gray, #767676) instead of the theme's purple | ![](accent-before.png) | ![](accent-after.png) |

The "after" executable reports version 0.0.0.0 / dev because it is a local
build; release builds carry the release version.

The title bar, backdrop, and accent shots are screen captures of the window's
DWM frame bounds taken while it was in front: `PrintWindow` leaves out the
caption buttons and the backdrop material. The acrylic shots blur whatever was
behind the window.

## Snap Layouts

Herdr UI keeps the system-drawn title bar on Windows, so its maximize button
is the system's own and hovering it opens the Snap Layouts flyout. There is
nothing to capture between before and after; the window's hit-test answer
over its caption shows it (`WM_NCHITTEST` sent to the main window at 150%
scale, 25 px below its top edge):

| Point | Answer |
| --- | --- |
| 30 px from the right edge | `HTCLOSE` (20) |
| 100 px from the right edge | `HTMAXBUTTON` (9) |
| 160 px from the right edge | `HTMINBUTTON` (8) |
| elsewhere on the caption | `HTCAPTION` (2) |

## Screen readers

What UI Automation (and so Narrator) sees in the main window, read with
`System.Windows.Automation`'s control view.

Before:

```text
Window '9950x3d: ~'
  Document ''        (text pattern: the rows on screen)
```

After:

```text
Window '9950x3d: ~'
  Button 'Sidebar'           (expand/collapse)
  Button 'Back'
  Button 'Forward'
  Tab ''                     (selection)
    TabItem '1'              (selection item, selected)
      Button 'Close tab'     (invoke)
  Button 'New tab'           (invoke)
  Button 'Split'             (invoke)
  Button 'Tab actions'       (invoke)
  Button 'GitHub account'    (invoke)
  Document 'Terminal'        (text pattern: the rows on screen)
```

Back and Forward show no invoke pattern here because there was no history to
go to, so both were disabled.

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
