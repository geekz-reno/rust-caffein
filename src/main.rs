#[cfg(target_os = "macos")]
mod activity;
#[cfg(target_os = "macos")]
mod app;
#[cfg(target_os = "macos")]
mod power;
mod session;
#[cfg(target_os = "macos")]
mod settings;

fn main() {
    #[cfg(target_os = "macos")]
    objc2::rc::autoreleasepool(|_| app::run());
    #[cfg(not(target_os = "macos"))]
    {
        eprintln!("Caffeine requires macOS 11 or later.");
        std::process::exit(1);
    }
}
