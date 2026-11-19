# Editor RS

A fast, native text editor for macOS, written in Rust.

## Features

- **Fast with large files.** Opens a 100 MB file in about 0.1 s, scrolls it smoothly, and draws only what is on screen. Lines tens of thousands of characters long stay responsive.
- **Never loses work.** Like Notepad++ and Windows 11 Notepad, closing the window or quitting never asks to save. Every tab, including unsaved and untitled ones, comes back next time, even after a crash or power cut. Only ⌘S writes to your file.
- **Safe FortiWeb backup editing.** Full-configuration backups open with their readable sections editable and their encrypted or binary sections locked. Saving keeps every byte you did not change, and refuses to write a backup whose structure no longer adds up.
- **FortiWeb CLI colors.** Commands, paths, options, on/off values, actions, severities, addresses, strings, certificates and `ENC` secrets are colored. `config`/`end` and `edit`/`next` blocks fold, and unmatched blocks are underlined.
- **Find and replace** with regular expressions, `$1` and `${name}` replacements, whole word, ignore case, in-selection, Find All, Count and Replace All (one undo step). Large searches run in the background.
- **Bookmarks** like Notepad++: toggle (⌘F2), next and previous (F2, ⇧F2), Bookmark All matches (⌥⌘F2), clear (⇧⌘F2), inverse, cut, copy, paste to, and remove bookmarked or unbookmarked lines.
- **Also:** tabs, multiple cursors, column selection (⌥-drag), input methods, word wrap, Go to Line, light and dark themes, and colors for JSON, Shell, Python, HTML, CSS, JavaScript, PHP and Diff.

## Install

1. Download `EditorRS-<version>.dmg` from the Releases page.
2. Open it and drag **Editor RS** to **Applications**.
3. Open Editor RS from Applications.

If the release is marked **unnotarized**, macOS will say it cannot verify the app the first time you open it. Open **System Settings → Privacy & Security**, scroll down, and click **Open Anyway** next to Editor RS. You only need to do this once.

You can check the download against the `.sha256` file:

```bash
shasum -a 256 -c EditorRS-<version>.dmg.sha256
```

## Build from source

Requires macOS 13 or later and Rust.

```bash
scripts/bundle.sh release          # builds target/release/Editor RS.app
scripts/make-dmg.sh                # builds the installer in dist/
scripts/perf-report.sh             # measures the speed targets
cargo test --workspace             # runs every test, including the app's UI scripts
```

## Where unsaved work is kept

`~/Library/Application Support/Editor RS/Session`. The folder is private to your account and excluded from Time Machine. **Clear Session Data…** in the app menu deletes it.

## Licence

Copyright © 2026 abemin. All rights reserved. Open-source components are listed in `THIRD_PARTY_NOTICES.md`.
