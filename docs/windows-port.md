# Windows port handoff

Laika now has Windows-aware storage paths, removable-drive discovery, Recycle
Bin deletion, File Explorer reveal, drive eject, default-app/browser launch,
Adobe preset discovery, SMB paths, OpenSSH behavior, packaging, and CI. GPUI and
wgpu provide the native window and DirectX 12 renderer.

## Before opening Codex on Windows

1. Install **Git for Windows**.
2. Install **Rustup** using the default `stable-x86_64-pc-windows-msvc`
   toolchain.
3. Install **Visual Studio 2022 Build Tools**, selecting **Desktop development
   with C++** and a Windows 10/11 SDK. Rust dependencies compile C/C++ code, so
   the linker and SDK are required even though Laika is Rust.
4. Clone/pull the repository onto an NTFS path. Do not reuse a `target/`
   directory copied from macOS.
5. In PowerShell from the repository root, run:

   ```powershell
   Set-ExecutionPolicy -Scope Process Bypass
   .\scripts\windows-bootstrap.ps1
   ```

The script checks the toolchain, runs the workspace build and tests, and stops
at the first actionable error. Pass `-Release` to also create
`dist\Laika-<version>-windows-x64.zip`.

For a faster first UI launch after the checks:

```powershell
cargo run --release -p laika-app
```

Application data lives under `%LOCALAPPDATA%\Laika`; the default catalog is
`%LOCALAPPDATA%\Laika\catalog.db`. Pictures created by Laika go under the
current user's `Pictures` folder.

## First native smoke test

Work through these in order and record the first failure with **Help → About
Laika → Copy Diagnostics**:

1. Launch, create/open the default catalog, close, and reopen it.
2. Import the sample RAWs and a JPEG; verify thumbnails and Develop rendering.
3. Import from an SD card, including copy, second copy, and eject.
4. Rate, flag, keyword, crop, and adjust a RAW; restart and verify persistence.
5. Export JPEG/PNG/TIFF/AVIF and run Export Everything with original copies.
6. Reveal a file, play a video, move a disposable file to the Recycle Bin, and
   restore it from there.
7. Build and preview a gallery; test Cloudflare only if `wrangler.cmd` is on
   `PATH`.
8. Test S3, Windows OpenSSH SFTP, and an SMB UNC path such as
   `\\server\photos` if those destinations are available.
9. Run `laika-render.exe --help`, then render one catalog photo.

## Expected platform differences

- Apple Photos integration and macOS ImageIO HEIC fallback are unavailable.
  HEIC support on Windows currently depends on formats decoded directly by the
  Rust image stack; unsupported files are reported rather than corrupted.
- Windows uses the Recycle Bin, File Explorer, DirectX 12/Vulkan, Credential
  Manager, `%LOCALAPPDATA%`, and UNC paths. UI text still includes some
  Lightroom/macOS shortcut glyphs; functional modifier handling comes from
  GPUI and should be verified during the native pass.
- Development zips are unsigned, so SmartScreen may require **More info → Run
  anyway**. Code signing and an installer are deliberately deferred until the
  native executable is stable.

## Useful commands for the Windows Codex task

```powershell
cargo check --workspace --all-targets --locked
cargo test --workspace --locked
cargo run --release -p laika-app
$env:RUST_BACKTRACE="1"; cargo run -p laika-app
```

GitHub Actions runs the same MSVC checks and produces a Windows zip on pushes
to `main` and tags. Use the Actions log as the authoritative compiler report if
the local environment fails before Rust compilation.
