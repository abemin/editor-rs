# Security

Editor RS opens and saves device configurations and full FortiWeb backups, so a bug that corrupts a file, leaks a secret or runs something it shouldn't is treated as a security issue.

## Reporting a vulnerability

Please report privately through GitHub: open the **Security** tab of this repository and choose **Report a vulnerability**. Do not open a public issue.

Include the Editor RS version (**Editor RS → About Editor RS**), your macOS version, and the steps to reproduce.

**Never attach a real configuration or backup.** They contain passwords, keys and network details. Describe the structure instead, or build a small sample with made-up names, documentation IP ranges (`192.0.2.0/24`, `198.51.100.0/24`, `2001:db8::/32`) and fake secrets. `scripts/check-cli.sh` and `scripts/verify-backup.sh` print only counts and structure, so their output is safe to share.

You'll get an acknowledgement within a week. Fixes are released as a new version on the Releases page and noted in the advisory.

## Supported versions

Only the latest release receives fixes.

## What Editor RS does with your files

- It makes no network connections and collects no telemetry.
- Unsaved work is kept in `~/Library/Application Support/Editor RS/Session`, readable only by your account and excluded from Time Machine. **Clear Session Data…** in the app menu deletes it.
- Encrypted and binary sections of FortiWeb backups are never decoded or shown; they are written back byte for byte.
