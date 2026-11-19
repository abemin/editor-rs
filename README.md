# Editor RS

**The first native macOS app built for editing and viewing Fortinet configuration files.**

Open a FortiGate, FortiWeb, FortiMail or FortiADC configuration and read it the way the device does: colored, folded by block, with mismatched blocks flagged. Edit full FortiWeb backups without breaking the encrypted parts, and get them back onto the device intact. Editor RS is written in Rust on AppKit, starts in a fraction of a second, and works offline. Your configurations never leave your Mac.

## Built for Fortinet configurations

- **One CLI language for every product.** Tested on FortiGate (FortiOS), FortiWeb, FortiMail and FortiADC configurations. Other products that use the same `config` / `edit` / `set` syntax color the same way. Files are recognized by their content, so `.conf`, `.cfg` and files with no extension all work.
- **Colors that follow the CLI's structure.** `config` paths, `edit` names, `set` / `unset` options, `enable` / `disable`, actions, severities, numbers, IPv4 and IPv6 addresses and networks, strings, certificates, `ENC` secrets, comments, and FortiMail's embedded logo and DLP data blocks all get their own color. Options missing from the documentation still color correctly.
- **Block folding and checking.** `config` / `end` and `edit` / `next` blocks fold, the matching line is easy to jump to, and unmatched blocks are underlined without ever stopping you from saving.
- **Safe FortiWeb backup editing.** Full-configuration backups open with their readable sections editable and their encrypted or binary sections locked. Saving keeps every byte you did not change, and refuses to write a backup whose structure no longer adds up. `scripts/verify-backup.sh` checks a real backup end to end.
- **Private by design.** No network access, no telemetry, no cloud sync. Unsaved work is kept in a folder only your account can read, excluded from Time Machine.

## A fast, everyday editor too

- **Fast with large files.** Opens a 100 MB file in about 0.1 s, scrolls it smoothly, and draws only what is on screen. Lines tens of thousands of characters long stay responsive.
- **Never loses work.** Like Notepad++ and Windows 11 Notepad, closing the window or quitting never asks to save. Every tab, including unsaved and untitled ones, comes back next time, even after a crash or power cut. Only ⌘S writes to your file. If auto-save ever stops working, the app asks before closing, as other editors do.
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
scripts/check-cli.sh <config>...   # checks coloring and folding of real configs, printing no contents
scripts/verify-backup.sh <backup>  # checks a real FortiWeb backup survives open, edit and save
cargo test --workspace             # runs every test, including the app's UI scripts
```

## Open configs with Editor RS by default

Select a configuration file in Finder, press ⌘I, choose **Editor RS** under **Open with**, and click **Change All…**. Repeat for each file extension you use, for example `.conf` and `.cfg`.

## Where unsaved work is kept

`~/Library/Application Support/Editor RS/Session`. The folder is private to your account and excluded from Time Machine. **Clear Session Data…** in the app menu deletes it.

## Trademarks

Fortinet, FortiGate, FortiWeb, FortiMail, FortiADC and FortiOS are trademarks of Fortinet, Inc. Editor RS is an independent project and is not affiliated with or endorsed by Fortinet.

## Licence

Copyright © 2026 abemin. All rights reserved. Open-source components are listed in `THIRD_PARTY_NOTICES.md`.
