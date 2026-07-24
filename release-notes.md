## What's Changed

### Features
- #77 Profiles: exposed/hidden tool lists now have tooltips with plain-language descriptions
- #78 Sync sidebar entry marked with "Planned" badge to signal deliberate non-implementation
- #79 Directory cards show plain-language explanations with "Create" button for missing directories

### Fixes
- #69 Dashboard: first-run "binary not found" error now shows install instructions, "Copy" and "Open Settings" buttons
- #70 Settings: Binary Path Override change shows restart notice with "Quit & Relaunch" button
- #71 Settings: Version panel shows error message instead of eternal spinner when version lookup fails
- #72 Dashboard: Configuration card no longer shows contradictory "Not Found" + "Parsed" badges together
- #73 Harnesses: Dry Run for Install now has a profile picker and doesn't fail without one
- #74 Harnesses: Dry Run button added for Uninstall
- #75 Harnesses: Install overwrite now shows confirmation alert before writing to harness config
- #76 App-wide: raw CLI stderr, exit codes and file paths no longer leak into user-facing error messages

### Internal
- CLIErrorFormatter: shared utility for mapping CLI errors to user-friendly messages
- Updated symaira-appkit pin to v0.2.2

### Closed Issues
- #69, #70, #71, #72, #73, #74, #75, #76, #77, #78, #79

**Full Changelog**: https://github.com/danieljustus/symaira-brain/compare/v0.2.2...v0.2.3
