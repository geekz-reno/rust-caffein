use objc2::rc::Retained;
use objc2_foundation::{NSString, NSUserDefaults};

pub const ACTIVATE: &str = "CAActivateAtLaunch";
pub const DURATION: &str = "CADefaultDuration";
pub const SUPPRESS: &str = "CASuppressLaunchMessage";
pub const DEACTIVATE: &str = "CADeactivateOnManualSleep";
pub const ACTIVITY: &str = "CAKeepAppsActive";

pub const DURATIONS: &[(&str, u32)] = &[
    ("Indefinitely", 0),
    ("5 minutes", 5),
    ("10 minutes", 10),
    ("15 minutes", 15),
    ("30 minutes", 30),
    ("1 hour", 60),
    ("2 hours", 120),
    ("5 hours", 300),
];

pub struct Settings(Retained<NSUserDefaults>);

impl Default for Settings {
    fn default() -> Self {
        Self(NSUserDefaults::standardUserDefaults())
    }
}

impl Settings {
    pub fn flag(&self, key: &str) -> bool {
        self.0.boolForKey(&NSString::from_str(key))
    }

    pub fn set_flag(&self, key: &str, value: bool) {
        self.0.setBool_forKey(value, &NSString::from_str(key));
    }

    pub fn duration(&self) -> u32 {
        let value = self.0.integerForKey(&NSString::from_str(DURATION));
        DURATIONS
            .iter()
            .find(|(_, minutes)| isize::try_from(*minutes) == Ok(value))
            .map_or(0, |(_, minutes)| *minutes)
    }

    pub fn set_duration(&self, minutes: u32) {
        self.0
            .setInteger_forKey(minutes as isize, &NSString::from_str(DURATION));
    }
}
