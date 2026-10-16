# Installs the latest stable Herdr UI release for the current user on Windows,
# served at https://herdrui.dev/install.ps1:
#
#   irm https://herdrui.dev/install.ps1 | iex
#
# It downloads the zip for this machine from GitHub Releases, checks it against
# the release's published SHA-256, installs it under
# %LOCALAPPDATA%\Programs\Herdr UI, adds its bin directory to the user PATH,
# and creates a Start menu shortcut. herdr-gpui.exe there is the windowed app;
# bin\herdr-gpui.exe is the console copy terminals and agents run. Releases
# older than that split had one executable, and the directory itself on PATH.
# The release's -setup.exe installs the same files to the same place. It never installs, starts or upgrades the Herdr
# daemon. Windows builds are experimental and never update themselves: run this
# again to update.
& {
  $ErrorActionPreference = 'Stop'
  $ProgressPreference = 'SilentlyContinue'
  [Net.ServicePointManager]::SecurityProtocol = [Net.ServicePointManager]::SecurityProtocol -bor [Net.SecurityProtocolType]::Tls12

  $repo = 'penso/herdr-gpui'
  $os = [Runtime.InteropServices.RuntimeInformation]::OSArchitecture
  $arch = switch ($os) {
    'X64' { 'x86_64' }
    'Arm64' { 'aarch64' }
    default { throw "Herdr UI has no Windows build for $os" }
  }

  # The latest release is always the newest stable one; betas are prereleases.
  $tag = (Invoke-RestMethod -UseBasicParsing "https://api.github.com/repos/$repo/releases/latest").tag_name
  if ($tag -notmatch '^v([1-9][0-9]{7}\.[1-9][0-9]*)$') { throw "Unexpected latest release tag: $tag" }
  $version = $Matches[1]

  $dest = Join-Path $env:LOCALAPPDATA 'Programs\Herdr UI'
  $exe = Join-Path $dest 'herdr-gpui.exe'
  $bin = Join-Path $dest 'bin'
  $cli = Join-Path $bin 'herdr-gpui.exe'
  # Windows will not overwrite a running executable, so an update needs the
  # installed app closed. Check before downloading and again before copying.
  $assertNotRunning = {
    $running = Get-Process -Name 'herdr-gpui' -ErrorAction SilentlyContinue | Where-Object { $_.Path -eq $exe -or $_.Path -eq $cli }
    if ($running) { throw "Herdr UI is running from $dest. Quit it, then run this command again; your Herdr sessions keep running." }
  }
  & $assertNotRunning

  $name = "Herdr-$version-$arch-pc-windows-msvc"
  $base = "https://github.com/$repo/releases/download/$tag"
  $tmp = Join-Path ([IO.Path]::GetTempPath()) ([IO.Path]::GetRandomFileName())
  New-Item -ItemType Directory -Path $tmp | Out-Null
  try {
    Write-Host "Downloading $name.zip"
    $zip = Join-Path $tmp "$name.zip"
    Invoke-WebRequest -UseBasicParsing "$base/$name.zip" -OutFile $zip
    $sums = Join-Path $tmp "$name.zip.sha256"
    Invoke-WebRequest -UseBasicParsing "$base/$name.zip.sha256" -OutFile $sums
    $expected = ((Get-Content -Raw $sums).Trim() -split '\s+')[0]
    $actual = (Get-FileHash -Algorithm SHA256 $zip).Hash
    if ($actual -ne $expected) { throw "Checksum mismatch for $name.zip" }

    Expand-Archive -Path $zip -DestinationPath $tmp
    & $assertNotRunning
    New-Item -ItemType Directory -Force -Path $dest | Out-Null
    Copy-Item -Recurse -Force -Path (Join-Path $tmp "$name\*") -Destination $dest
  } finally {
    Remove-Item -Recurse -Force -ErrorAction SilentlyContinue $tmp
  }

  # The console copy goes on PATH, replacing the directory an older install
  # put there: the windowed app is one a shell does not wait for. A release
  # without the copy keeps the old layout.
  $onPath, $stale = if (Test-Path -PathType Leaf $cli) { $bin, $dest } else { $dest, $bin }
  $entries = @(([Environment]::GetEnvironmentVariable('Path', 'User') -split ';') | Where-Object { $_ })
  $kept = @($entries | Where-Object { $_.TrimEnd('\') -ne $stale })
  if (-not ($kept | Where-Object { $_.TrimEnd('\') -eq $onPath })) { $kept += $onPath }
  if (($kept -join ';') -ne ($entries -join ';')) {
    [Environment]::SetEnvironmentVariable('Path', ($kept -join ';'), 'User')
    Write-Host "Added $onPath to your user PATH; open a new terminal to use herdr-gpui."
  }

  $shortcut = Join-Path ([Environment]::GetFolderPath('Programs')) 'Herdr UI.lnk'
  $link = (New-Object -ComObject WScript.Shell).CreateShortcut($shortcut)
  $link.TargetPath = $exe
  $link.WorkingDirectory = $dest
  $link.Save()

  Write-Host "Installed Herdr UI $version to $exe. Start it from the Start menu (Herdr UI) or run: herdr-gpui"
  if (-not (Get-Command herdr -ErrorAction SilentlyContinue)) {
    Write-Host 'Herdr UI needs the Herdr daemon, which is not on PATH. Install it from https://herdr.dev/'
  }
}
