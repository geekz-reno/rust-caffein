//! Timer-independent session state with an absolute wall-clock deadline.

use std::time::{Duration, SystemTime};

#[derive(Debug, Default)]
enum State {
    #[default]
    Inactive,
    Indefinite,
    Until(SystemTime),
}

#[derive(Debug, Default)]
pub struct Session {
    state: State,
}

impl Session {
    /// Starts or replaces a session. Zero minutes means no deadline.
    pub fn activate(&mut self, minutes: u32, now: SystemTime) {
        self.state = if minutes == 0 {
            State::Indefinite
        } else {
            State::Until(now + Duration::from_secs(u64::from(minutes) * 60))
        };
    }

    pub fn deactivate(&mut self) {
        self.state = State::Inactive;
    }

    /// Reports explicit state; call `expire` to reconcile a passed deadline.
    pub fn is_active(&self) -> bool {
        !matches!(self.state, State::Inactive)
    }

    /// Returns no duration when inactive or indefinite, and zero when overdue.
    pub fn remaining(&self, now: SystemTime) -> Option<Duration> {
        match self.state {
            State::Until(deadline) => Some(deadline.duration_since(now).unwrap_or_default()),
            State::Inactive | State::Indefinite => None,
        }
    }

    /// Deactivates at or after the deadline, including after sleep or clock jumps.
    /// Returns true only for the transition from active to expired.
    pub fn expire(&mut self, now: SystemTime) -> bool {
        if matches!(self.state, State::Until(deadline) if now >= deadline) {
            self.deactivate();
            true
        } else {
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Session;
    use std::time::{Duration, SystemTime, UNIX_EPOCH};

    fn start() -> SystemTime {
        UNIX_EPOCH + Duration::from_secs(1_700_000_000)
    }

    #[test]
    fn starts_inactive_and_deactivation_is_idempotent() {
        let mut session = Session::default();
        assert!(!session.is_active());
        assert_eq!(session.remaining(start()), None);
        assert!(!session.expire(start()));
        session.deactivate();
        assert!(!session.is_active());
        assert_eq!(session.remaining(start()), None);
    }

    #[test]
    fn timed_session_expires_at_inclusive_nanosecond_boundary() {
        let mut session = Session::default();
        let now = start();
        let deadline = now + Duration::from_secs(60);
        session.activate(1, now);
        assert!(session.is_active());
        assert_eq!(session.remaining(now), Some(Duration::from_secs(60)));

        let just_before = deadline - Duration::from_nanos(1);
        assert_eq!(
            session.remaining(just_before),
            Some(Duration::from_nanos(1))
        );
        assert!(!session.expire(just_before));
        assert!(session.is_active());

        assert_eq!(session.remaining(deadline), Some(Duration::ZERO));
        assert!(session.expire(deadline));
        assert!(!session.is_active());
        assert_eq!(session.remaining(deadline), None);
        assert!(!session.expire(deadline));
    }

    #[test]
    fn waking_after_deadline_expires_without_intermediate_timer_ticks() {
        let mut session = Session::default();
        let now = start();
        session.activate(5, now);
        let wake = now + Duration::from_secs(8 * 60 * 60);
        assert_eq!(session.remaining(wake), Some(Duration::ZERO));
        assert!(session.expire(wake));
        assert!(!session.is_active());
        assert_eq!(session.remaining(wake), None);
        assert!(!session.expire(wake + Duration::from_secs(1)));
    }

    #[test]
    fn reactivation_replaces_the_old_deadline() {
        let mut session = Session::default();
        let now = start();
        session.activate(1, now);
        session.activate(2, now + Duration::from_secs(30));
        let old_deadline = now + Duration::from_secs(60);
        assert!(!session.expire(old_deadline));
        assert_eq!(
            session.remaining(old_deadline),
            Some(Duration::from_secs(90))
        );
        assert!(session.expire(now + Duration::from_secs(150)));
    }

    #[test]
    fn indefinite_activation_removes_a_previous_deadline() {
        let mut session = Session::default();
        let now = start();
        session.activate(1, now);
        session.activate(0, now + Duration::from_secs(10));
        assert!(session.is_active());
        assert_eq!(session.remaining(now), None);
        assert!(!session.expire(now + Duration::from_secs(365 * 24 * 60 * 60)));
        assert!(session.is_active());
        session.deactivate();
        assert!(!session.is_active());
    }

    #[test]
    fn timed_activation_replaces_indefinite_and_cancel_clears_deadline() {
        let mut session = Session::default();
        let now = start();
        session.activate(0, now);
        session.activate(1, now);
        assert_eq!(session.remaining(now), Some(Duration::from_secs(60)));
        session.deactivate();
        assert_eq!(session.remaining(now), None);
        assert!(!session.expire(now + Duration::from_secs(60)));
        session.activate(0, now);
        assert!(!session.expire(now + Duration::from_secs(120)));
        assert!(session.is_active());
    }

    #[test]
    fn wall_clock_changes_compare_against_the_original_deadline() {
        let mut session = Session::default();
        let now = start();
        session.activate(1, now);
        let earlier = now - Duration::from_secs(30);
        assert_eq!(session.remaining(earlier), Some(Duration::from_secs(90)));
        assert!(!session.expire(earlier));
        assert!(session.expire(now + Duration::from_secs(120)));
        assert!(!session.is_active());
    }

    #[test]
    fn maximum_minutes_does_not_overflow_integer_conversion() {
        let mut session = Session::default();
        let now = start();
        session.activate(u32::MAX, now);
        assert_eq!(
            session.remaining(now),
            Some(Duration::from_secs(257_698_037_700))
        );
        assert!(!session.expire(now + Duration::from_secs(60)));
    }
}
