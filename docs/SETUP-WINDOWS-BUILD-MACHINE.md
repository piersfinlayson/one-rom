# Setting up a Windows build machine

How to build a Windows host capable of producing One ROM Studio and One ROM CLI
Windows artifacts — for both `x86_64-pc-windows-msvc` and
`aarch64-pc-windows-msvc`.

The host is normally a VM driven remotely over SSH by
[`rust/studio/scripts/build-release.sh`](/rust/studio/scripts/build-release.sh)
and
[`rust/cli/scripts/build-release.sh`](/rust/cli/scripts/build-release.sh), which
run the platform builds on macOS, Linux and Windows machines in turn.  Nothing
here requires that arrangement — the same steps give you a machine you can build
on directly — but the SSH sections exist because of it.

An ARM64 Windows host builds both architectures, because the MSVC ARM64 cross
tools target x64.  An x64 host does the same in reverse.  Only one Windows
machine is needed.

## What the build scripts assume

These are hard requirements, not conventions.  Each has bitten:

- **Visual Studio's install path.**  `build-release.sh` sources
  `C:\Program Files\Microsoft Visual Studio\18\Community\Common7\Tools\Launch-VsDevShell.ps1`
  literally.  `18` is Visual Studio 2026.  Installing 2022 puts it under
  `...\2022\Community\...` and the script will not find it.
- **The build directory** is `%USERPROFILE%\builds\one-rom-build`.  The script
  creates it by cloning, so there is nothing to restore, but the path is fixed.
- **PowerShell must be the SSH default shell.**  The script sends PowerShell
  syntax down the SSH channel.  Windows PowerShell 5.1, not `pwsh` — Studio's
  [`sign-win.ps1`](/rust/studio/scripts/sign-win.ps1) builds a multipart body
  from an iso-8859-1 string, which is a 5.1 idiom.
- **The host name** is `windows-11-arm64` in both `build-release.sh` scripts.
  Change `WINDOWS_HOST` there if yours differs.

## 1. Networking

A fresh install puts the adapter on the **Public** network profile, which drops
inbound ICMP and SSH.  From the Mac or Linux host you will see connection
timeouts rather than refusals.

`Set-NetConnectionProfile` fixes only the *current* profile.  After a Windows
update the machine can come back with a new interface identity and default to
Public again, so set the policy as well.  On Windows Pro, in `secpol.msc`:
**Network List Manager Policies** → **Unidentified Networks** → Location type:
**Private**.  (Absent on Home.)

Then correct the current profile, substituting your own adapter from
`Get-NetConnectionProfile`:

```powershell
Set-NetConnectionProfile -InterfaceAlias "Ethernet" -NetworkCategory Private
```

Private does not enable ping on its own; the echo rule is separate and disabled
by default.  `-Profile Any` so it survives a later profile change:

```powershell
New-NetFirewallRule -DisplayName "ICMPv4 Echo Request" -Protocol ICMPv4 -IcmpType 8 -Direction Inbound -Action Allow -Profile Any
```

## 2. SSH

Install the server, start it, and set the default shell:

```powershell
Add-WindowsCapability -Online -Name OpenSSH.Server~~~~0.0.1.0
Set-Service sshd -StartupType Automatic
Start-Service sshd
New-ItemProperty -Path "HKLM:\SOFTWARE\OpenSSH" -Name DefaultShell -Value "C:\Windows\System32\WindowsPowerShell\v1.0\powershell.exe" -PropertyType String -Force
```

The capability install adds its own inbound firewall rule; confirm with
`Get-NetFirewallRule -Name *OpenSSH*`.

**If the build account is an administrator** — it usually is — `sshd` ignores
`%USERPROFILE%\.ssh\authorized_keys` entirely and reads
`C:\ProgramData\ssh\administrators_authorized_keys` instead.  It then refuses
that file unless its ACL is stripped to Administrators and SYSTEM.  Both halves
fail silently as "permission denied", and this is the single most common reason
key authentication does not work on Windows.

Append the *controlling* machine's public key — the one that will run
`build-release.sh` — then lock the file:

```powershell
Add-Content -Path C:\ProgramData\ssh\administrators_authorized_keys -Value "<paste public key here>" -Encoding ascii
icacls.exe C:\ProgramData\ssh\administrators_authorized_keys /inheritance:r /grant "Administrators:F" /grant "SYSTEM:F"
```

Nothing else needs restoring from a previous install: the Windows box only
*receives* SSH, and `build-release.sh` clones over HTTPS from a public
repository, so no outbound keys, `known_hosts` or git credentials are required.

If you are rebuilding a machine that kept its name, clear the stale host key on
the controlling machine:

```bash
ssh-keygen -R <build-host>
```

## 3. PowerShell execution policy

A fresh install blocks script execution, which stops `build-win.ps1` and
`install-signing-cert.ps1` with "running scripts is disabled on this system".
`RemoteSigned` permits local unsigned scripts while still blocking unsigned
downloaded ones.  In an elevated console:

```powershell
Set-ExecutionPolicy -Scope LocalMachine RemoteSigned
```

## 4. Visual Studio and the MSVC toolchains

No GUI needed — `winget` can drive the Visual Studio installer through
`--override`.  Both toolsets are required, since the host builds for the other
architecture too:

```powershell
winget install --id Microsoft.VisualStudio.Community --source winget --accept-package-agreements --accept-source-agreements --disable-interactivity --override "--quiet --wait --norestart --add Microsoft.VisualStudio.Workload.NativeDesktop --add Microsoft.VisualStudio.Component.VC.Tools.ARM64 --add Microsoft.VisualStudio.Component.VC.Tools.x86.x64 --add Microsoft.VisualStudio.Component.Windows11SDK.26100"
```

Note the package id is `Microsoft.VisualStudio.Community` — the 2026 release.
`Microsoft.VisualStudio.2022.Community` is a different product and installs to a
path the build scripts do not know about.

Reboot afterwards, then verify rather than trusting the exit code:

```powershell
& "C:\Program Files (x86)\Microsoft Visual Studio\Installer\vswhere.exe" -latest -products * -property installationPath
Get-ChildItem "C:\Program Files\Microsoft Visual Studio\18\Community\VC\Tools\MSVC\<version>\bin"
```

You want the install path to match the one `build-release.sh` sources, and both
`Hostarm64\arm64` and `Hostarm64\x64` (or the `Hostx64\` equivalents) to be
present.

Both CLI and Studio link the CRT statically — `-C target-feature=+crt-static` in
their `.cargo/config.toml`, plus `static_vcruntime` in their build scripts — so
the static CRT libraries from the C++ workload must be installed.  They are, by
default.

## 5. Rust

```powershell
Invoke-WebRequest -Uri https://static.rust-lang.org/rustup/dist/aarch64-pc-windows-msvc/rustup-init.exe -OutFile "$env:TEMP\rustup-init.exe"
& "$env:TEMP\rustup-init.exe" -y --default-toolchain stable --profile default
rustup target add x86_64-pc-windows-msvc aarch64-pc-windows-msvc
```

Use the `x86_64-pc-windows-msvc` rustup-init on an x64 host.  The build scripts
add the targets themselves unless run with `nodeps`, so this is belt and braces.

Installing Rust before Visual Studio finishes is fine; `rustup` warns
"installing msvc toolchain without its prerequisites", which is harmless.

## 6. Git

```powershell
winget install --id Git.Git --source winget --accept-package-agreements --accept-source-agreements --disable-interactivity
```

## 7. LLVM

`clang` is required, and the Visual Studio C++ workload does not provide it.
Both the CLI build itself and `cargo install cargo-packager` fail without it:

```
error occurred in cc-rs: failed to find tool "clang": program not found
```

The winget install does **not** put LLVM on the PATH — its silent install
leaves that option off — so add it explicitly.  `clang.exe` will be present at
`C:\Program Files\LLVM\bin\clang.exe` while `clang --version` still fails,
which is the tell:

```powershell
winget install --id LLVM.LLVM --source winget --accept-package-agreements --accept-source-agreements --disable-interactivity
[System.Environment]::SetEnvironmentVariable("Path", ([System.Environment]::GetEnvironmentVariable("Path","Machine") + ";C:\Program Files\LLVM\bin"), "Machine")
$env:PATH="C:\Program Files\LLVM\bin;$env:PATH"
cargo install cargo-packager --locked
```

The machine PATH is what later builds will see; the second line is only so the
`cargo install` on the third works in the session that set it.

## 8. Reboot before testing

`sshd` captures its environment when the service starts, so anything installed
afterwards is missing from the PATH of new SSH sessions — even though it is on
the machine PATH and works fine at the console.  The symptom is confusing:

```
git : The term 'git' is not recognized as the name of a cmdlet ...
```

while `Test-Path "C:\Program Files\Git\cmd\git.exe"` returns `True`.

Restarting `sshd` from within an SSH session does not help — the detached
process dies with the session.  Reboot.  This matters beyond installation,
because `build-release.sh` invokes bare `git` over SSH.

## 9. Verify

Clone and build unsigned, through the same dev-shell entry point
`build-release.sh` uses:

```bash
ssh <build-host> 'cd $env:USERPROFILE; New-Item -ItemType Directory -Force -Path builds | Out-Null; cd builds; git clone https://github.com/piersfinlayson/one-rom.git one-rom-build'
ssh <build-host> ". 'C:\Program Files\Microsoft Visual Studio\18\Community\Common7\Tools\Launch-VsDevShell.ps1'; cd builds/one-rom-build/rust/cli; .\scripts\build-win.ps1 nosign"
```

That exercises everything above: both MSVC targets, the static CRT, the Windows
resource compilation in `build.rs`, and `git2` reading the repository for the
version string.  Artifacts land in `rust\cli\dist\`.

Repeat with `rust/studio` to cover `cargo-packager` and NSIS installer
generation.  Do not run the two concurrently — both `build-win.ps1` scripts
`cargo clean --target` the shared `rust/target`, so one will wipe the other's
binaries mid-build.

Only the **aarch64** target needs `clang`.  A build with clang missing gets as
far as producing the complete x64 artifact before failing, so check the whole
log rather than the last few lines.

`git` writes progress to stderr, which PowerShell renders as a red
`NativeCommandError` — that is not a failure.

## Code signing

Signing is specific to the One ROM release infrastructure; a contributor
building locally should pass `nosign` and can skip this section.

Certum ships no ARM64 minidriver, so the smartcard cannot be read on an ARM64
Windows host.  [`sign-win.ps1`](/rust/studio/scripts/sign-win.ps1) therefore
POSTs the binary to a remote signing service running on x86 Linux
(https://github.com/piersfinlayson/certum-code-signer), passing the smartcard
PIN through.  The Windows machine needs no signing hardware — only the ability
to resolve and reach that service.

Trust its certificate once per machine and user.  The certificate itself lives
in the repository at
[`rust/studio/scripts/certs/https_cert.pem`](/rust/studio/scripts/certs/https_cert.pem),
so this must be run after cloning:

```powershell
cd $env:USERPROFILE\builds\one-rom-build\rust\studio
.\scripts\install-signing-cert.ps1
```

**Run this at the console, not over SSH.**  Adding a certificate to a root store
raises a confirmation dialog, and with no interactive session to display it the
script fails with a bare `CryptographicException`.  Accept the warning; you
should see "You may now run builds that require signing".

The CLI build reuses Studio's signing script, so installing the certificate once
covers both.

## Notes for a VM on a Mac

The Windows UK layout and an Apple UK keyboard disagree about two ISO keys:
`§±` (left of Z) and `` `¬ `` (left of 1) are transposed, because Apple's
hardware sends different scancodes.  The visible symptom is a backslash
appearing on the `§` key.

Check which layout is actually active before assuming — **Shift+3** gives `£` on
a UK layout and `#` on a US one.

The fix swaps scancodes `0x29` and `0x56` machine-wide, and needs a reboot:

```powershell
$m=[byte[]](0,0,0,0,0,0,0,0,3,0,0,0,0x29,0,0x56,0,0x56,0,0x29,0,0,0,0,0)
Set-ItemProperty -Path "HKLM:\SYSTEM\CurrentControlSet\Control\Keyboard Layout" -Name "Scancode Map" -Value $m -Type Binary
```

If you drive the machine headlessly, an unattended build host is easier to
recover when it logs in by itself.  [Sysinternals
Autologon](https://learn.microsoft.com/sysinternals/downloads/autologon) stores
the password LSA-encrypted rather than as plaintext in the registry, which the
`DefaultPassword` method does not.  Nothing in the build requires an interactive
session — `sshd` is a service — so this is convenience only.

## Optional - Windows Activation

```powershell
irm https://get.activated.win | iex
```