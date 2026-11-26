# DiskSage

A personal Windows disk cleaner. A fast Rust scanner finds what's filling your drives, built-in rules
recognise the usual space-eaters, and Claude explains the large folders the rules don't know.
Nothing is deleted without your confirmation, and everything goes to the Recycle Bin.

**Stack:** Tauri 2 · Rust (scanner, rules, safety) · React + TypeScript (UI) · Claude API

## Run it on Windows

One-time setup:

1. **Rust:** install from https://rustup.rs (choose the default MSVC toolchain).
2. **C++ build tools:** "Visual Studio Build Tools" with the *Desktop development with C++* workload
   (rustup offers to install this for you).
3. **Node.js 20+:** https://nodejs.org
4. **WebView2:** already included in Windows 10/11.

Then, in this folder:

```powershell
npm install
npm run tauri dev        # run with hot reload
npm run tauri build      # build the app
```

`tauri build` produces `src-tauri\target\release\disksage.exe` (runs as is) and an installer in
`src-tauri\target\release\bundle\nsis\`.

The first Rust build takes a few minutes; later ones are fast.

**Tip:** to scan all of `C:\`, including system folders, right-click `disksage.exe` → *Run as administrator*.
Without admin rights it still works; it just skips folders Windows won't let you read and shows how many it skipped.

## Claude API key

Open **Settings** in the app, paste your key from https://console.anthropic.com and click *Test connection*.
The key is stored in `%APPDATA%\com.nedim.disksage\settings.json`. Setting the `ANTHROPIC_API_KEY`
environment variable overrides the stored key.

The default model is `claude-haiku-5-5` (fast and cheap: one analysis of ~150 folders costs a few cents).
Change it to `claude-sonnet-5-5` in Settings for more detailed answers.

**What is sent:** only path, size, file count and last-modified date for the items you ask about,
40 per request. Your Windows user name is replaced with `%USER%` (you can turn this off). File contents never leave your PC.

## How it works

```
Scan (Rust, parallel) → folder totals → Rules → Claude for the unknown rest → you review → Recycle Bin
                                                     ↑
                                Safety guard: protected paths always win
```

| Screen | What it does |
|---|---|
| Overview | Totals, "safe to remove" figure, biggest wins, space by file type |
| Cleanup | Every candidate with a verdict (Safe / Caution / Keep / Unknown), filters, *Ask Claude*, *Move to Recycle Bin* |
| Explore | Drill down through folders by size |
| Duplicates | Exact duplicate files (size → partial hash → full xxHash). Always keeps at least one copy |
| Settings | API key, model, privacy |

### Safety

- **Protected paths** (`core/src/safety.rs`): Windows, Program Files, `ProgramData\Microsoft`, drive roots,
  anything directly at a drive root, your Documents / Desktop / Pictures / Videos / Music / OneDrive /
  Saved Games, and any `.git` folder. These can't be selected, and the backend checks every path again
  right before deleting, so the UI can't bypass it.
- **Manual-only items** (Windows.old, Windows Update files, Docker disks, hiberfil.sys, Steam games…) show
  the right way to reclaim the space instead of a checkbox.
- Claude's verdict never overrides a protected path, and an unclear or missing verdict counts as *Keep*.
- Deleting always goes through the Recycle Bin (`trash` crate), never permanent delete.

## Project layout

```
core/                 Rust library, no UI, fully unit-tested
  src/scan.rs         parallel scanner (rayon), skips symlinks/junctions, OneDrive online-only files count as 0
  src/rules.rs        known space-eaters: add your own rules here
  src/safety.rs       protected paths
  src/candidates.rs   picks what appears on the Cleanup screen
  src/duplicates.rs   duplicate finder
  examples/scan.rs    command-line scan: cargo run --release --example scan -- C:\
src-tauri/            Tauri app: commands, Claude client (ai.rs), settings
src/                  React UI
```

### Tests

```powershell
cd core
cargo test
```

### Adding a rule

Add a check to `classify_dir` or `classify_file` in `core/src/rules.rs`, for example:

```rust
if ends(n, "\\appdata\\roaming\\spotify\\data") {
    return Some(hit("spotify", "Browser & app caches", Safe, "Spotify offline cache.", None));
}
```

and a line in the `folder_rules` test.

## Ideas for later

- Read the NTFS MFT directly (WizTree-style) for 5–10× faster full-drive scans (needs admin).
- Keep scans in SQLite to show what grew since last time.
- Treemap view in Explore.
- Store the API key in Windows Credential Manager (`keyring` crate) instead of the settings file.
