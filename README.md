# Caffeine

Native macOS menu bar rewrite of [domzilla/Caffeine](https://github.com/domzilla/Caffeine/tree/f938edb3aa76c3332584c2975b0bac4dec1837e7), written in Rust with AppKit and IOKit. No Swift, webview, background worker, or subprocess for sleep prevention.

## Build and run

Requires macOS 11 or later, Rust 1.85 or later, and Apple's Command Line Tools (`xcode-select --install`).

```sh
./scripts/build-app.sh
open "target/release/Caffeine.app"
```

Drag `target/release/Caffeine.app` into Applications to install. The script generates the application icon and ad-hoc signs the bundle. Public distribution requires your own Developer ID signing and notarization.

To build for another Mac architecture, install the corresponding Rust target first:

```sh
rustup target add x86_64-apple-darwin
./scripts/build-app.sh x86_64-apple-darwin
```

Explicit-target bundles appear under `target/<triple>/release/`. The script also accepts `aarch64-apple-darwin`, `CARGO_BUILD_TARGET`, and `CARGO_TARGET_DIR`. Use the bundle, not `cargo run`, to load translations and cup images.

## Use

- Left-click the cup to toggle sleep prevention. A full cup means active; an empty cup means inactive.
- Right-click, Control-click, or Command-click for the menu.
- **Activate for** starts or replaces a session: indefinitely, 5, 10, 15, or 30 minutes, or 1, 2, or 5 hours.
- **Preferences** controls the default duration, activation at app launch, the welcome window, deactivation on manual sleep, and optional activity simulation.
- The menu shows remaining time when opened. The timeout continues while menus are tracking.
- **Quit** releases the sleep assertion. Closing Preferences leaves the menu bar app running.

Preferences use the separate `net.caffeine-app.rust` defaults domain; the Swift app's settings are not changed or automatically imported. The welcome window appears at launch until its checkbox is cleared. To launch at login, add the app under System Settings → General → Login Items.

### Sleep and permissions

Normal operation holds one `PreventUserIdleDisplaySleep` assertion. It prevents idle display/system sleep, not explicit sleep, lid closure, critical-battery sleep, screen locking, or every screen saver policy. No Accessibility permission is needed for this mode.

**Keep apps active** requests Accessibility permission and checks session idle time every 30 seconds, with 3 seconds of scheduling tolerance. After at least 90 idle seconds, it submits a mouse-move event at the existing cursor position. It does not move the cursor to another position, send keys, or guarantee a particular app's presence status. macOS and managed security policies can block synthetic input.

Enable permission in System Settings → Privacy & Security → Accessibility, then return to Caffeine's Preferences or toggle the option again. Revoking permission stops activity checks. Sleep prevention continues without it.

Sleep and user-session notifications release the assertion and stop activity checks. On wake/session return, Caffeine checks the wall-clock deadline before resuming. With **Deactivate when device goes to sleep manually** enabled, a sleep notification cancels the session instead.

### Upstream differences

- Persistent, RAII-owned assertion replaces the upstream 10-second refresh loop and 8-second assertion timeout.
- A timed session uses one deadline timer. Remaining time is computed when opening the menu, rather than waking every second.
- Inactive and indefinite sessions without activity simulation have no app timers.
- Native AppKit controls replace SwiftUI/Combine. Activity checks query Core Graphics directly instead of allocating an I/O Registry property dictionary.
- Original cup images and all 13 upstream language resources are retained. Rewrite-specific diagnostic messages and About text are English.
- Sparkle is not used: the upstream signed feed distributes the Swift app, not this Rust build. **Upstream project…** opens its source repository; it does not check or install Rust updates. Automatic Rust updates require a separate signed release feed and signing identity.

These are architectural changes, not measured speed or memory improvements against Swift.

## Verification

```sh
cargo test --locked
./scripts/build-app.sh
codesign --verify --strict "target/release/Caffeine.app"
```

Eight deterministic regression tests cover deadline boundaries, wake after expiration, duration replacement, cancellation, indefinite sessions, clock changes, and integer overflow.

A temporary native smoke executable also exercised the preferences window and controls, status-button toggles, scheduled expiration, assertion creation/release observed through `pmset`, and simulated sleep/session notifications. It was removed after verification. Screen capture was unavailable in the build environment, so pixel-level layout, VoiceOver announcements, real hardware sleep, and permission-granted event delivery were not verified. Runtime checks ran on Apple Silicon; Intel and older macOS runtime compatibility remain unverified.

## License

MIT. Upstream copyrights and license are preserved in [LICENSE](LICENSE), including bundled images and translations.
