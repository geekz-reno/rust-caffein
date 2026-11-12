# Changelog

## Unreleased

- Kept the application name as Caffeine in the bundle and native interface.
- Reduced Preferences from 680 × 500 to 520 × 354 points with tighter control spacing and a concise header.
- Added an original, scalable coffee-cup logo to Preferences (`resources/Logo.pdf`).
- Verified native window bounds, logo loading, and duration selection with a temporary smoke executable.
- Replaced the About alert with a compact 400 × 280-point window using the same logo as Preferences.
- Added selectable Rust rewrite credit for geekz.reno@gmail.com while retaining upstream copyrights.
- Verified About opens, closes, and reopens without closing Preferences.

## 0.1.0 — 2026-11-11

- Rewrote Caffeine as a native Rust/AppKit menu bar app.
- Retained duration selection, launch preferences, manual-sleep policy, optional activity simulation, cup assets, and 13 language resources.
- Replaced repeated sleep-assertion refreshes with one RAII-owned IOKit assertion.
- Replaced per-second countdown updates with deadline-based, one-shot expiration that also reconciles on wake.
- Suspended assertions and activity simulation outside the active user session and during sleep.
- Added permission-aware, stationary activity events and explicit native errors.
- Added a reproducible, ad-hoc-signed app bundle build and deadline regression tests.
- Isolated preferences from the original app and omitted its incompatible Swift/Sparkle update feed.

### Build evidence

The initial Apple Silicon release executable was 356,000 bytes, excluding bundle resources. Eight deadline tests and a native control/assertion smoke run passed. The release bundle passed strict code-signature verification. No comparative performance benchmark was run; visual capture and permission-granted activity delivery were unavailable.
