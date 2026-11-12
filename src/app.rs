use std::cell::{Cell, OnceCell, RefCell};
use std::time::SystemTime;

use objc2::rc::Retained;
use objc2::runtime::{ProtocolObject, Sel};
use objc2::{AnyThread, DefinedClass, MainThreadOnly, define_class, msg_send, sel};
use objc2_app_kit::*;
use objc2_foundation::{
    MainThreadMarker, NSBundle, NSNotification, NSObject, NSObjectProtocol, NSPoint, NSRect,
    NSRunLoop, NSRunLoopCommonModes, NSSize, NSString, NSTimer, NSURL,
};

use crate::{
    activity,
    power::PowerAssertion,
    session::Session,
    settings::{self, Settings},
};

fn tr(key: &str) -> Retained<NSString> {
    NSBundle::mainBundle().localizedStringForKey_value_table(&NSString::from_str(key), None, None)
}

fn accessibility(key: &str) -> Retained<NSString> {
    NSBundle::mainBundle().localizedStringForKey_value_table(
        &NSString::from_str(key),
        None,
        Some(&NSString::from_str("Accessibility")),
    )
}

fn rect(x: f64, y: f64, width: f64, height: f64) -> NSRect {
    NSRect::new(NSPoint::new(x, y), NSSize::new(width, height))
}

#[derive(Default)]
struct State {
    session: Session,
    power: Option<PowerAssertion>,
    timeout: Option<Retained<NSTimer>>,
    activity: Option<Retained<NSTimer>>,
}

struct Ivars {
    state: RefCell<State>,
    settings: Settings,
    status: OnceCell<Retained<NSStatusItem>>,
    images: OnceCell<(Retained<NSImage>, Retained<NSImage>)>,
    window: OnceCell<Retained<NSWindow>>,
    about_window: OnceCell<Retained<NSWindow>>,
    session_active: Cell<bool>,
    awake: Cell<bool>,
}

impl Default for Ivars {
    fn default() -> Self {
        Self {
            state: RefCell::default(),
            settings: Settings::default(),
            status: OnceCell::new(),
            images: OnceCell::new(),
            window: OnceCell::new(),
            about_window: OnceCell::new(),
            session_active: Cell::new(true),
            awake: Cell::new(true),
        }
    }
}

define_class!(
    // SAFETY: NSObject has no subclass invariants. All AppKit state stays on the main thread.
    #[unsafe(super = NSObject)]
    #[thread_kind = MainThreadOnly]
    #[ivars = Ivars]
    struct Delegate;

    unsafe impl NSObjectProtocol for Delegate {}

    unsafe impl NSApplicationDelegate for Delegate {
        #[unsafe(method(applicationDidFinishLaunching:))]
        fn launched(&self, _: &NSNotification) {
            self.setup();
        }

        #[unsafe(method(applicationWillTerminate:))]
        fn terminating(&self, _: &NSNotification) {
            self.deactivate();
            // SAFETY: Remove this live observer before the delegate is released.
            unsafe { NSWorkspace::sharedWorkspace().notificationCenter().removeObserver(self); }
            if let Some(status) = self.ivars().status.get() {
                NSStatusBar::systemStatusBar().removeStatusItem(status);
            }
        }

        #[unsafe(method(applicationDidBecomeActive:))]
        fn became_active(&self, _: &NSNotification) {
            self.sync_activity();
        }
    }

    impl Delegate {
        #[unsafe(method(statusClicked:))]
        fn clicked(&self, _: &NSButton) {
            let app = NSApplication::sharedApplication(self.mtm());
            if app.currentEvent().is_some_and(|event| {
                event.r#type() == NSEventType::RightMouseUp ||
                    event.modifierFlags().contains(NSEventModifierFlags::Control) ||
                    event.modifierFlags().contains(NSEventModifierFlags::Command)
            }) {
                self.show_menu();
            } else if self.ivars().state.borrow().session.is_active() {
                self.deactivate();
            } else {
                self.activate(self.ivars().settings.duration());
            }
        }

        #[unsafe(method(activateFor:))]
        fn activate_for(&self, sender: &NSMenuItem) {
            self.activate(sender.tag() as u32);
        }

        #[unsafe(method(timeoutFired:))]
        fn timeout_fired(&self, _: &NSTimer) {
            if self.ivars().state.borrow_mut().session.expire(SystemTime::now()) {
                self.deactivate();
            } else {
                self.schedule_timeout();
            }
        }

        #[unsafe(method(activityFired:))]
        fn activity_fired(&self, _: &NSTimer) {
            if !activity::permitted(false) {
                self.stop_activity();
                return;
            }
            if let Err(error) = activity::simulate_if_idle() {
                self.stop_activity();
                self.error(&error);
            }
        }

        #[unsafe(method(showPreferences:))]
        fn preferences(&self, _: &NSObject) { self.show_preferences(); }

        #[unsafe(method(changeFlag:))]
        fn change_flag(&self, sender: &NSButton) {
            let key = match sender.tag() {
                0 => settings::ACTIVATE,
                1 => settings::DEACTIVATE,
                2 => settings::SUPPRESS,
                3 => settings::ACTIVITY,
                _ => return,
            };
            let checked = sender.state() == 1;
            self.ivars().settings.set_flag(key, if key == settings::SUPPRESS { !checked } else { checked });
            if key == settings::ACTIVITY {
                if checked && !activity::permitted(true) {
                    self.error("Keep apps active needs Accessibility permission. Enable Caffeine in System Settings → Privacy & Security → Accessibility. Sleep prevention needs no permission.");
                }
                self.sync_activity();
            }
        }

        #[unsafe(method(changeDuration:))]
        fn change_duration(&self, sender: &NSPopUpButton) {
            self.ivars().settings.set_duration(sender.selectedTag() as u32);
        }

        #[unsafe(method(closePreferences:))]
        fn close_preferences(&self, _: &NSObject) {
            if let Some(window) = self.ivars().window.get() { window.close(); }
        }

        #[unsafe(method(showAbout:))]
        fn about(&self, _: &NSObject) {
            self.show_about();
        }

        #[unsafe(method(closeAbout:))]
        fn close_about(&self, _: &NSObject) {
            if let Some(window) = self.ivars().about_window.get() { window.close(); }
        }

        #[unsafe(method(openProject:))]
        fn project(&self, _: &NSObject) {
            if let Some(url) = NSURL::URLWithString(&NSString::from_str("https://github.com/domzilla/Caffeine")) {
                NSWorkspace::sharedWorkspace().openURL(&url);
            }
        }

        #[unsafe(method(quit:))]
        fn quit(&self, _: &NSObject) {
            NSApplication::sharedApplication(self.mtm()).terminate(None);
        }

        #[unsafe(method(willSleep:))]
        fn will_sleep(&self, _: &NSNotification) {
            self.ivars().awake.set(false);
            if self.ivars().settings.flag(settings::DEACTIVATE) {
                self.deactivate();
            } else {
                self.suspend();
            }
        }

        #[unsafe(method(didWake:))]
        fn did_wake(&self, _: &NSNotification) {
            self.ivars().awake.set(true);
            self.resume();
        }

        #[unsafe(method(sessionResigned:))]
        fn session_resigned(&self, _: &NSNotification) {
            self.ivars().session_active.set(false);
            self.suspend();
        }

        #[unsafe(method(sessionBecameActive:))]
        fn session_became_active(&self, _: &NSNotification) {
            self.ivars().session_active.set(true);
            self.resume();
        }
    }
);

impl Delegate {
    fn new(mtm: MainThreadMarker) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(Ivars::default());
        // SAFETY: NSObject initializer matches the declared superclass.
        unsafe { msg_send![super(this), init] }
    }

    fn setup(&self) {
        let status =
            NSStatusBar::systemStatusBar().statusItemWithLength(NSVariableStatusItemLength);
        if let Some(button) = status.button(self.mtm()) {
            // SAFETY: Selectors below match the methods declared on Delegate.
            unsafe {
                button.setTarget(Some(self));
                button.setAction(Some(sel!(statusClicked:)));
                button.sendActionOn(NSEventMask::LeftMouseUp | NSEventMask::RightMouseUp);
                button.setAccessibilityLabel(Some(&NSString::from_str("Caffeine")));
                button.setAccessibilityIdentifier(Some(&NSString::from_str("Caffeine.status")));
            }
        }
        self.ivars()
            .status
            .set(status)
            .expect("status initialized once");
        if let (Some(inactive), Some(active)) =
            (self.load_image("inactive"), self.load_image("active"))
        {
            self.ivars()
                .images
                .set((inactive, active))
                .expect("images initialized once");
        }
        let center = NSWorkspace::sharedWorkspace().notificationCenter();
        // SAFETY: Each selector accepts exactly one NSNotification. Observer lives through app.run().
        unsafe {
            for (name, selector) in [
                (NSWorkspaceWillSleepNotification, sel!(willSleep:)),
                (NSWorkspaceDidWakeNotification, sel!(didWake:)),
                (
                    NSWorkspaceSessionDidResignActiveNotification,
                    sel!(sessionResigned:),
                ),
                (
                    NSWorkspaceSessionDidBecomeActiveNotification,
                    sel!(sessionBecameActive:),
                ),
            ] {
                center.addObserver_selector_name_object(self, selector, Some(name), None);
            }
        }
        self.update_icon();
        if self.ivars().settings.flag(settings::ACTIVATE) {
            self.activate(self.ivars().settings.duration());
        }
        if !self.ivars().settings.flag(settings::SUPPRESS) {
            self.show_preferences();
        }
    }

    fn load_image(&self, name: &str) -> Option<Retained<NSImage>> {
        let path = NSBundle::mainBundle().pathForResource_ofType(
            Some(&NSString::from_str(name)),
            Some(&NSString::from_str("png")),
        )?;
        let image = NSImage::initWithContentsOfFile(NSImage::alloc(), &path)?;
        image.setTemplate(true);
        image.setSize(NSSize::new(22.0, 20.0));
        Some(image)
    }

    fn activate(&self, minutes: u32) {
        self.deactivate();
        if self.ivars().session_active.get() && self.ivars().awake.get() {
            match PowerAssertion::acquire() {
                Ok(assertion) => self.ivars().state.borrow_mut().power = Some(assertion),
                Err(error) => {
                    self.error(&error);
                    return;
                }
            }
        }
        self.ivars()
            .state
            .borrow_mut()
            .session
            .activate(minutes, SystemTime::now());
        self.schedule_timeout();
        self.sync_activity();
        self.update_icon();
    }

    fn deactivate(&self) {
        let mut state = self.ivars().state.borrow_mut();
        state.session.deactivate();
        state.power = None;
        for timer in [state.timeout.take(), state.activity.take()]
            .into_iter()
            .flatten()
        {
            timer.invalidate();
        }
        drop(state);
        self.update_icon();
    }

    fn timer(&self, seconds: f64, selector: Sel, repeats: bool) -> Retained<NSTimer> {
        // SAFETY: Callers pass a declared timer callback with one NSTimer parameter.
        unsafe {
            let timer = NSTimer::timerWithTimeInterval_target_selector_userInfo_repeats(
                seconds.max(0.001),
                self,
                selector,
                None,
                repeats,
            );
            if repeats {
                timer.setTolerance(3.0);
            }
            NSRunLoop::mainRunLoop().addTimer_forMode(&timer, NSRunLoopCommonModes);
            timer
        }
    }

    fn schedule_timeout(&self) {
        let mut state = self.ivars().state.borrow_mut();
        if let Some(timer) = state.timeout.take() {
            timer.invalidate();
        }
        if let Some(remaining) = state.session.remaining(SystemTime::now()) {
            state.timeout = Some(self.timer(remaining.as_secs_f64(), sel!(timeoutFired:), false));
        }
    }

    fn stop_activity(&self) {
        if let Some(timer) = self.ivars().state.borrow_mut().activity.take() {
            timer.invalidate();
        }
    }

    fn sync_activity(&self) {
        self.stop_activity();
        let enabled = self.ivars().state.borrow().power.is_some()
            && self.ivars().settings.flag(settings::ACTIVITY)
            && activity::permitted(false);
        if enabled {
            self.ivars().state.borrow_mut().activity =
                Some(self.timer(30.0, sel!(activityFired:), true));
        }
    }

    fn suspend(&self) {
        self.ivars().state.borrow_mut().power = None;
        self.stop_activity();
        self.update_icon();
    }

    fn resume(&self) {
        let expired = self
            .ivars()
            .state
            .borrow_mut()
            .session
            .expire(SystemTime::now());
        if expired {
            self.deactivate();
            return;
        }
        if !self.ivars().session_active.get() || !self.ivars().awake.get() {
            return;
        }
        let needs_power = {
            let state = self.ivars().state.borrow();
            state.session.is_active() && state.power.is_none()
        };
        if needs_power {
            match PowerAssertion::acquire() {
                Ok(assertion) => self.ivars().state.borrow_mut().power = Some(assertion),
                Err(error) => {
                    self.deactivate();
                    self.error(&error);
                    return;
                }
            }
        }
        self.schedule_timeout();
        self.sync_activity();
        self.update_icon();
    }

    fn update_icon(&self) {
        let Some(button) = self.ivars().status.get().and_then(|s| s.button(self.mtm())) else {
            return;
        };
        let active = self.ivars().state.borrow().power.is_some();
        if let Some((inactive_image, active_image)) = self.ivars().images.get() {
            button.setImage(Some(if active { active_image } else { inactive_image }));
        } else {
            button.setTitle(&NSString::from_str(if active { "● ☕" } else { "☕" }));
        }
        let value = accessibility(if active {
            "Active, sleep prevented"
        } else {
            "Inactive, sleep allowed"
        });
        button.setToolTip(Some(&value));
        unsafe {
            button.setAccessibilityValue(Some(&value));
        }
    }

    fn menu_item(&self, title: &NSString, action: Option<Sel>, key: &str) -> Retained<NSMenuItem> {
        // SAFETY: Actions match declared Delegate callbacks. AppKit keeps target non-owning.
        unsafe {
            let item = NSMenuItem::initWithTitle_action_keyEquivalent(
                NSMenuItem::alloc(self.mtm()),
                title,
                action,
                &NSString::from_str(key),
            );
            item.setTarget(Some(self));
            item
        }
    }

    fn show_menu(&self) {
        let expired = self
            .ivars()
            .state
            .borrow_mut()
            .session
            .expire(SystemTime::now());
        if expired {
            self.deactivate();
        }
        let menu = NSMenu::new(self.mtm());
        menu.setAutoenablesItems(false);
        let state = self.ivars().state.borrow();
        if state.session.is_active() {
            let label = if let Some(remaining) = state.session.remaining(SystemTime::now()) {
                let seconds = remaining.as_secs();
                if seconds >= 3600 {
                    format!("{:02}:{:02}", seconds / 3600, (seconds % 3600) / 60)
                } else if seconds > 60 {
                    tr("%d minutes")
                        .to_string()
                        .replace("%d", &(seconds / 60).to_string())
                } else {
                    tr("%d seconds")
                        .to_string()
                        .replace("%d", &seconds.to_string())
                }
            } else {
                tr("Caffeine is active").to_string()
            };
            let info = self.menu_item(&NSString::from_str(&label), None, "");
            info.setEnabled(false);
            menu.addItem(&info);
            menu.addItem(&NSMenuItem::separatorItem(self.mtm()));
        }
        drop(state);
        let activate = self.menu_item(&tr("Activate for"), None, "");
        let durations = NSMenu::new(self.mtm());
        for &(label, minutes) in settings::DURATIONS {
            let item = self.menu_item(&tr(label), Some(sel!(activateFor:)), "");
            item.setTag(minutes as isize);
            let accessible = accessibility("Activate: %@")
                .to_string()
                .replace("%@", &tr(label).to_string());
            item.setAccessibilityLabel(Some(&NSString::from_str(&accessible)));
            durations.addItem(&item);
        }
        activate.setSubmenu(Some(&durations));
        menu.addItem(&activate);
        menu.addItem(&NSMenuItem::separatorItem(self.mtm()));
        for (label, action, key) in [
            ("Preferences...", sel!(showPreferences:), ","),
            ("About Caffeine", sel!(showAbout:), ""),
            ("Upstream project…", sel!(openProject:), ""),
            ("Quit", sel!(quit:), "q"),
        ] {
            menu.addItem(&self.menu_item(&tr(label), Some(action), key));
        }
        if let Some(button) = self.ivars().status.get().and_then(|s| s.button(self.mtm())) {
            menu.popUpMenuPositioningItem_atLocation_inView(
                None,
                NSPoint::new(0.0, 0.0),
                Some(&button),
            );
        }
    }

    fn label(&self, view: &NSView, text: &str, frame: NSRect) {
        let label = NSTextField::wrappingLabelWithString(&tr(text), self.mtm());
        label.setFrame(frame);
        view.addSubview(&label);
    }

    fn add_logo(&self, view: &NSView, frame: NSRect) {
        if let Some(path) = NSBundle::mainBundle().pathForResource_ofType(
            Some(&NSString::from_str("Logo")),
            Some(&NSString::from_str("pdf")),
        ) {
            if let Some(image) = NSImage::initWithContentsOfFile(NSImage::alloc(), &path) {
                let logo = NSImageView::initWithFrame(NSImageView::alloc(self.mtm()), frame);
                logo.setImage(Some(&image));
                logo.setAccessibilityElement(false);
                view.addSubview(&logo);
            }
        }
    }

    fn show_about(&self) {
        if self.ivars().about_window.get().is_none() {
            // SAFETY: This retained window disables AppKit's release-on-close behavior.
            let window = unsafe {
                NSWindow::initWithContentRect_styleMask_backing_defer(
                    NSWindow::alloc(self.mtm()),
                    rect(0.0, 0.0, 400.0, 280.0),
                    NSWindowStyleMask::Titled | NSWindowStyleMask::Closable,
                    NSBackingStoreType::Buffered,
                    false,
                )
            };
            unsafe {
                window.setReleasedWhenClosed(false);
            }
            window.setTitle(&tr("About Caffeine"));
            window.center();
            let view = window.contentView().expect("About content view");
            self.add_logo(&view, rect(20.0, 196.0, 64.0, 64.0));
            let title = NSTextField::labelWithString(&NSString::from_str("Caffeine"), self.mtm());
            title.setFont(Some(&NSFont::boldSystemFontOfSize(22.0)));
            title.setFrame(rect(100.0, 228.0, 280.0, 30.0));
            view.addSubview(&title);
            self.label(
                &view,
                concat!("Version ", env!("CARGO_PKG_VERSION"), " · MIT license"),
                rect(100.0, 202.0, 280.0, 22.0),
            );
            self.label(&view, "Rust rewrite by", rect(20.0, 166.0, 360.0, 18.0));
            self.label(
                &view,
                "geekz.reno@gmail.com",
                rect(20.0, 142.0, 360.0, 22.0),
            );
            let credits = NSTextField::wrappingLabelWithString(
                &NSString::from_str(
                    "© 2006–2018 Tomas Franzén, Lighthead Software\n© 2018 Michael Jones, IntelliScape Computer Solutions\n© 2022 Dominic Rodemer",
                ),
                self.mtm(),
            );
            credits.setFont(Some(&NSFont::systemFontOfSize(11.0)));
            credits.setFrame(rect(20.0, 62.0, 360.0, 68.0));
            view.addSubview(&credits);
            for (text, action, x, width, key) in [
                ("Upstream project…", sel!(openProject:), 20.0, 160.0, ""),
                ("Close", sel!(closeAbout:), 290.0, 90.0, "\r"),
            ] {
                // SAFETY: Actions match Delegate selectors and the delegate outlives the window.
                let button = unsafe {
                    NSButton::buttonWithTitle_target_action(
                        &tr(text),
                        Some(self),
                        Some(action),
                        self.mtm(),
                    )
                };
                button.setFrame(rect(x, 18.0, width, 28.0));
                button.setKeyEquivalent(&NSString::from_str(key));
                view.addSubview(&button);
            }
            self.ivars()
                .about_window
                .set(window)
                .expect("About initialized once");
        }
        #[allow(deprecated)]
        NSApplication::sharedApplication(self.mtm()).activateIgnoringOtherApps(true);
        self.ivars()
            .about_window
            .get()
            .unwrap()
            .makeKeyAndOrderFront(None);
    }

    fn show_preferences(&self) {
        if self.ivars().window.get().is_none() {
            // SAFETY: The retained window disables AppKit's release-on-close behavior.
            let window = unsafe {
                NSWindow::initWithContentRect_styleMask_backing_defer(
                    NSWindow::alloc(self.mtm()),
                    rect(0.0, 0.0, 520.0, 354.0),
                    NSWindowStyleMask::Titled | NSWindowStyleMask::Closable,
                    NSBackingStoreType::Buffered,
                    false,
                )
            };
            unsafe {
                window.setReleasedWhenClosed(false);
            }
            window.setTitle(&tr("Welcome to Caffeine"));
            window.center();
            let view = window.contentView().expect("window content view");
            self.add_logo(&view, rect(20.0, 270.0, 64.0, 64.0));
            let title = NSTextField::labelWithString(&NSString::from_str("Caffeine"), self.mtm());
            title.setFont(Some(&NSFont::boldSystemFontOfSize(22.0)));
            title.setFrame(rect(100.0, 302.0, 400.0, 30.0));
            view.addSubview(&title);
            self.label(
                &view,
                "Keep your Mac awake.",
                rect(100.0, 276.0, 400.0, 22.0),
            );
            self.label(
                &view,
                "Right-click (or ⌃-click) the menu bar icon to show the Caffeine menu.",
                rect(20.0, 224.0, 480.0, 38.0),
            );
            self.label(&view, "Default duration:", rect(20.0, 188.0, 220.0, 24.0));
            let picker = NSPopUpButton::initWithFrame_pullsDown(
                NSPopUpButton::alloc(self.mtm()),
                rect(254.0, 188.0, 246.0, 26.0),
                false,
            );
            for &(label, minutes) in settings::DURATIONS {
                picker.addItemWithTitle(&tr(label));
                if let Some(item) = picker.lastItem() {
                    item.setTag(minutes as isize);
                }
            }
            picker.selectItemWithTag(self.ivars().settings.duration() as isize);
            unsafe {
                picker.setTarget(Some(self));
                picker.setAction(Some(sel!(changeDuration:)));
                picker.setAccessibilityLabel(Some(&tr("Default duration:")));
                view.addSubview(&picker);
            }
            for (tag, text, key) in [
                (0, "Activate when starting Caffeine", settings::ACTIVATE),
                (
                    1,
                    "Deactivate when device goes to sleep manually",
                    settings::DEACTIVATE,
                ),
                (
                    2,
                    "Show this message when starting Caffeine",
                    settings::SUPPRESS,
                ),
                (3, "Keep apps active", settings::ACTIVITY),
            ] {
                let button = unsafe {
                    NSButton::checkboxWithTitle_target_action(
                        &tr(text),
                        Some(self),
                        Some(sel!(changeFlag:)),
                        self.mtm(),
                    )
                };
                button.setTag(tag);
                let flag = self.ivars().settings.flag(key);
                button.setState(isize::from(if key == settings::SUPPRESS {
                    !flag
                } else {
                    flag
                }));
                button.setFrame(rect(20.0, 156.0 - tag as f64 * 26.0, 480.0, 22.0));
                view.addSubview(&button);
            }
            self.label(
                &view,
                "Prevents apps from becoming inactive and the screen saver from starting.",
                rect(40.0, 42.0, 460.0, 32.0),
            );
            for (text, action, x, key) in [
                ("Quit", sel!(quit:), 20.0, "q"),
                ("Close", sel!(closePreferences:), 410.0, "\r"),
            ] {
                let button = unsafe {
                    NSButton::buttonWithTitle_target_action(
                        &tr(text),
                        Some(self),
                        Some(action),
                        self.mtm(),
                    )
                };
                button.setFrame(rect(x, 10.0, 90.0, 28.0));
                button.setKeyEquivalent(&NSString::from_str(key));
                view.addSubview(&button);
            }
            self.ivars()
                .window
                .set(window)
                .expect("window initialized once");
        }
        #[allow(deprecated)]
        NSApplication::sharedApplication(self.mtm()).activateIgnoringOtherApps(true);
        self.ivars()
            .window
            .get()
            .unwrap()
            .makeKeyAndOrderFront(None);
    }

    fn error(&self, message: &str) {
        eprintln!("Caffeine: {message}");
        self.message("Caffeine", message);
    }

    fn message(&self, title: &str, message: &str) {
        let alert = NSAlert::new(self.mtm());
        alert.setMessageText(&NSString::from_str(title));
        alert.setInformativeText(&NSString::from_str(message));
        alert.addButtonWithTitle(&NSString::from_str("OK"));
        alert.runModal();
    }
}

pub fn run() {
    let mtm = MainThreadMarker::new().expect("AppKit must run on main thread");
    let app = NSApplication::sharedApplication(mtm);
    app.setActivationPolicy(NSApplicationActivationPolicy::Accessory);
    let delegate = Delegate::new(mtm);
    app.setDelegate(Some(ProtocolObject::from_ref(&*delegate)));
    app.run();
}
