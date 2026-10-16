# Windows QA prompt

A prompt for a coding agent running on a Windows machine with this repository
checked out, to test the windowed app, its console copy, and the installer.
Copy everything in the block below into the agent.

```text
You're on a Windows machine with the herdr-gpui repo checked out. Test the `windows` branch,
which changes how Herdr UI meets the Windows console. Read AGENTS.md first, and don't modify or
push anything unless I ask; report what you find.

What the branch does:
- Release builds of herdr-gpui.exe are GUI-subsystem (windowed), so opening the app shows no console.
- Packaging adds bin\herdr-gpui.exe, a console-subsystem copy of the same build (only the PE
  subsystem field differs). Shells wait for it and see its exit code. With no command it starts
  the windowed app beside it and returns.
- Every child process (git, ssh, wsl.exe…) is created with CREATE_NO_WINDOW via
  herdr_client::new_command, so none flash a console.
- scripts/release/herdr-gpui.iss is a per-user Inno Setup installer; scripts/release/windows-installer.py
  builds and smoke-tests it.

Steps (PowerShell, from the repo root):
1. git fetch origin; git checkout windows; git pull
2. cargo build --release -p herdr-gpui
3. Remove-Item -Recurse -Force dist -ErrorAction SilentlyContinue; mkdir dist
   python scripts\release\package-windows.py 20261015.1 x86_64-pc-windows-msvc target\release\herdr-gpui.exe dist LICENSE
4. Find ISCC.exe (Inno Setup 6 is installed; try "C:\Program Files (x86)\Inno Setup 6\ISCC.exe"
   and "$env:LOCALAPPDATA\Programs\Inno Setup 6\ISCC.exe"), then:
   python scripts\release\windows-installer.py build 20261015.1 x86_64-pc-windows-msvc dist\Herdr-20261015.1-x86_64-pc-windows-msvc.zip dist <ISCC path>
5. python scripts\release\windows-installer.py smoke dist\Herdr-20261015.1-x86_64-pc-windows-msvc-setup.exe
   (silent install to a temp dir, checks files, subsystems, console output and PATH, then uninstalls)

Then, using computer use if you have it (otherwise give me exact steps and I'll do them):
6. Run the setup exe normally. A signed release names its publisher in any
   SmartScreen prompt; a local build is unsigned and needs More info → Run
   anyway. Keep "Add herdr-gpui to PATH".
7. Open "Herdr UI" from the Start menu. Take screenshots right away: does any console window appear,
   even briefly? Use the app a bit (open a repo/worktree so git runs) and watch for console flashes.
8. In a NEW PowerShell window: `herdr-gpui --help` (output prints, prompt returns after it),
   `herdr-gpui --build-info; $LASTEXITCODE` (expect 0), and `herdr-gpui` alone (app opens, prompt
   returns immediately).
9. Uninstall from Settings → Apps → Herdr UI; confirm %LOCALAPPDATA%\Programs\Herdr UI is gone and
   `herdr-gpui` is no longer found in a new terminal.

Report: Windows version and architecture, the output of each command (full error text for any
failure, including ISCC line numbers), and for steps 6–9 what you saw, with screenshots.
```

On an ARM64 machine, replace `x86_64-pc-windows-msvc` with `aarch64-pc-windows-msvc`
throughout.
