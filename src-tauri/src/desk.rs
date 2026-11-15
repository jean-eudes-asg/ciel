// Whether the user is at their desk (Ciel window → Chat → Speak up): Ciel
// speaks without being asked only then (speakup.rs).
//
// Three things say so. None is watched: each is read at the moment Ciel has
// something to say, and nothing runs in between.
//   * The session: locked, under a screen saver or another user's, no one is
//     there, whatever else says.
//   * What the user told Ciel, « I'm leaving » or « I'm back »: it holds for
//     the day. « I'm leaving » also ends when they next speak to Ciel.
//   * Otherwise the days and the hours they gave in the settings: one span,
//     or several, each with its own days.

use std::sync::Mutex;
use std::time::{Duration, Instant};

use chrono::{Datelike, Local, NaiveDate, NaiveDateTime, Timelike};
use serde_json::Value;

use crate::settings::SpeakUp;

/// What the user said last, the day they said it, and when.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Told {
    here: bool,
    day: NaiveDate,
    at: Instant,
}

static TOLD: Mutex<Option<Told>> = Mutex::new(None);

/// « I'm leaving », then a word of thanks to Ciel's goodbye: still leaving.
const SAME_BREATH: Duration = Duration::from_secs(120);
const DAY: u32 = 24 * 60;

/// "HH:MM" as minutes of the day.
fn minutes(clock: &str) -> Option<u32> {
    let (h, m) = clock.trim().split_once(':')?;
    let (h, m): (u32, u32) = (h.trim().parse().ok()?, m.trim().parse().ok()?);
    (h < 24 && m < 60).then_some(h * 60 + m)
}

/// More spans than anyone needs; past it, the rest are left out.
const MAX_SPANS: usize = 12;

/// Some days (0 Monday … 6 Sunday), from one hour to another.
#[derive(Clone, Debug, PartialEq)]
struct Span {
    days: Vec<u8>,
    from: u32,
    to: u32,
}

/// When the user is at their desk: one span or several.
#[derive(Clone, Debug, PartialEq)]
pub struct Hours {
    spans: Vec<Span>,
}

impl Hours {
    /// As the settings say: their periods, or the one span of `days`, `from`
    /// and `to` when they have none. An hour that doesn't read is the usual
    /// one in that single span; a period whose hours don't read is left out.
    pub fn of(prefs: &SpeakUp) -> Hours {
        if prefs.periods.is_empty() {
            return Hours {
                spans: vec![Span { days: prefs.days.clone(), from: minutes(&prefs.from).unwrap_or(9 * 60), to: minutes(&prefs.to).unwrap_or(18 * 60) }],
            };
        }
        let spans = prefs
            .periods
            .iter()
            .filter_map(|p| Some(Span { days: p.days.clone(), from: minutes(&p.from)?, to: minutes(&p.to)? }))
            .take(MAX_SPANS)
            .collect();
        Hours { spans }
    }

    /// How many minutes ago the span `at` is in began, the latest begun if
    /// two hold; none outside them all.
    pub fn since_start(&self, at: NaiveDateTime) -> Option<u32> {
        self.spans.iter().filter_map(|s| s.since_start(at)).min()
    }

    /// How many minutes until the next span begins today; none once they all
    /// have, or on a day off.
    pub fn until_start(&self, at: NaiveDateTime) -> Option<u32> {
        self.spans.iter().filter_map(|s| s.until_start(at)).min()
    }
}

impl Span {
    /// How many minutes ago the span `at` is in began; none outside it.
    /// Hours that end before they start run past midnight, and the small
    /// hours then belong to the day before. The same hour twice is a whole
    /// day, from that hour.
    fn since_start(&self, at: NaiveDateTime) -> Option<u32> {
        let day = at.weekday().num_days_from_monday() as u8;
        let minute = at.hour() * 60 + at.minute();
        let on = |d: u8| self.days.contains(&d);
        if self.from < self.to {
            (on(day) && (self.from..self.to).contains(&minute)).then(|| minute - self.from)
        } else if on(day) && minute >= self.from {
            Some(minute - self.from)
        } else if on((day + 6) % 7) && minute < self.to {
            Some(minute + DAY - self.from)
        } else {
            None
        }
    }

    /// How many minutes until today's span begins; none once it has, or on
    /// a day off.
    fn until_start(&self, at: NaiveDateTime) -> Option<u32> {
        let day = at.weekday().num_days_from_monday() as u8;
        let minute = at.hour() * 60 + at.minute();
        (self.days.contains(&day) && minute < self.from).then(|| self.from - minute)
    }
}

/// At their desk or not, from the three signs.
fn decide(locked: bool, told: Option<Told>, hours: &Hours, now: NaiveDateTime) -> bool {
    if locked {
        return false;
    }
    match told.filter(|t| t.day == now.date()) {
        Some(told) => told.here,
        None => hours.since_start(now).is_some(),
    }
}

/// The user is at their desk, as far as Ciel can tell right now.
pub fn there(prefs: &SpeakUp) -> bool {
    let told = *TOLD.lock().unwrap();
    decide(crate::platform::session_locked(), told, &Hours::of(prefs), Local::now().naive_local())
}

/// The user said they are leaving, or that they are back.
pub fn tell(here: bool) {
    *TOLD.lock().unwrap() = Some(Told { here, day: Local::now().date_naive(), at: Instant::now() });
}

/// What they told Ciel after `told`, speaking to it again at `now`: someone
/// who speaks to Ciel is there, so « I'm leaving » is over.
fn after_speaking(told: Option<Told>, now: Instant) -> Option<Told> {
    told.filter(|t| t.here || now.duration_since(t.at) < SAME_BREATH)
}

/// The user spoke to Ciel (voice/mod.rs).
pub fn spoke() {
    let mut told = TOLD.lock().unwrap();
    *told = after_speaking(*told, Instant::now());
}

/// The brain's `desk` tool (voice/brain.rs): what goes back to it, and that
/// something changed.
pub fn for_brain(input: &Value) -> (String, bool) {
    match input["here"].as_bool() {
        Some(true) => {
            tell(true);
            ("Noted: the user is at their desk for the rest of the day, and Ciel speaks up while the session is unlocked.".into(), true)
        }
        Some(false) => {
            tell(false);
            ("Noted: the user is away. Ciel says nothing on its own until they speak to it again or say they are back; what comes in meanwhile waits in the island.".into(), true)
        }
        None => ("Say whether the user is here: true or false.".into(), false),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(day: u32, clock: &str) -> NaiveDateTime {
        // November 2026: the 9th is a Monday.
        let (h, m) = clock.split_once(':').unwrap();
        NaiveDate::from_ymd_opt(2026, 11, day).unwrap().and_hms_opt(h.parse().unwrap(), m.parse().unwrap(), 0).unwrap()
    }

    fn hours(days: &[u8], from: &str, to: &str) -> Hours {
        Hours::of(&SpeakUp { days: days.to_vec(), from: from.into(), to: to.into(), ..SpeakUp::default() })
    }

    #[test]
    fn the_days_and_hours_given_are_the_desks() {
        let week = hours(&[0, 1, 2, 3, 4], "09:00", "18:00");
        assert_eq!(week.since_start(at(9, "09:00")), Some(0));
        assert_eq!(week.since_start(at(13, "17:59")), Some(539));
        for (day, clock) in [(9, "08:59"), (9, "18:00"), (14, "10:00"), (15, "10:00")] {
            assert_eq!(week.since_start(at(day, clock)), None, "{day} {clock}");
        }
        // Before they begin, on a day at the desk only.
        assert_eq!(week.until_start(at(9, "08:15")), Some(45));
        assert_eq!(week.until_start(at(9, "09:00")), None);
        assert_eq!(week.until_start(at(14, "08:15")), None);
        // An hour that doesn't read is the usual one.
        assert_eq!(hours(&[0], "soon", "25:00"), hours(&[0], "09:00", "18:00"));
    }

    #[test]
    fn hours_past_midnight_belong_to_the_day_they_began() {
        // Friday evening into Saturday's small hours.
        let night = hours(&[4], "20:00", "02:00");
        assert_eq!(night.since_start(at(13, "20:00")), Some(0));
        assert_eq!(night.since_start(at(14, "01:30")), Some(330));
        assert_eq!(night.since_start(at(14, "02:00")), None);
        assert_eq!(night.since_start(at(13, "01:30")), None, "Thursday's night is not Friday's");
        assert_eq!(night.since_start(at(14, "21:00")), None, "Saturday is a day off");
        // The same hour twice: the whole day, from that hour.
        let all = hours(&[0], "06:00", "06:00");
        assert_eq!(all.since_start(at(9, "06:00")), Some(0));
        assert_eq!(all.since_start(at(10, "05:59")), Some(DAY - 1));
        assert_eq!(all.since_start(at(10, "06:00")), None);
    }

    #[test]
    fn several_periods_each_with_its_days_make_the_week() {
        use crate::settings::Period;
        let period = |days: &[u8], from: &str, to: &str| Period { days: days.to_vec(), from: from.into(), to: to.into() };
        // Early mornings and evenings on weekdays, the weekend all day.
        let prefs = SpeakUp {
            periods: vec![
                period(&[0, 1, 2, 3, 4], "06:30", "07:00"),
                period(&[0, 1, 2, 3, 4], "17:45", "00:00"),
                period(&[5, 6], "00:00", "00:00"),
            ],
            ..SpeakUp::default()
        };
        let week = Hours::of(&prefs);
        assert_eq!(week.since_start(at(9, "06:30")), Some(0));
        assert_eq!(week.since_start(at(9, "06:59")), Some(29));
        assert_eq!(week.since_start(at(9, "07:00")), None);
        assert_eq!(week.since_start(at(9, "12:00")), None);
        assert_eq!(week.since_start(at(9, "17:45")), Some(0));
        assert_eq!(week.since_start(at(9, "23:59")), Some(374));
        assert_eq!(week.since_start(at(10, "00:30")), None, "Tuesday's small hours are not Monday's evening");
        // Friday evening, then Saturday from midnight, all of it.
        assert_eq!(week.since_start(at(13, "23:00")), Some(315));
        assert_eq!(week.since_start(at(14, "00:00")), Some(0));
        assert_eq!(week.since_start(at(14, "13:00")), Some(13 * 60));
        assert_eq!(week.since_start(at(15, "23:59")), Some(DAY - 1));
        assert_eq!(week.since_start(at(16, "06:00")), None, "Monday again");
        // The next span of the day.
        assert_eq!(week.until_start(at(9, "06:00")), Some(30));
        assert_eq!(week.until_start(at(9, "08:00")), Some(585));
        assert_eq!(week.until_start(at(9, "18:00")), None);
        // A period whose hours don't read is left out, not made the usual one.
        let odd = SpeakUp { periods: vec![period(&[0], "soon", "18:00")], ..SpeakUp::default() };
        assert_eq!(Hours::of(&odd).since_start(at(9, "10:00")), None);
    }

    #[test]
    fn a_locked_session_wins_then_what_was_told_today_then_the_hours() {
        let week = hours(&[0, 1, 2, 3, 4], "09:00", "18:00");
        let told = |here: bool, day: u32| Some(Told { here, day: at(day, "00:00").date(), at: Instant::now() });
        let monday_ten = at(9, "10:00");
        let monday_night = at(9, "22:00");
        assert!(decide(false, None, &week, monday_ten));
        assert!(!decide(false, None, &week, monday_night));
        // Locked: no one is there, even told « I'm back ».
        assert!(!decide(true, None, &week, monday_ten));
        assert!(!decide(true, told(true, 9), &week, monday_ten));
        // « I'm leaving » in the middle of the day; « I'm back » in the evening.
        assert!(!decide(false, told(false, 9), &week, monday_ten));
        assert!(decide(false, told(true, 9), &week, monday_night));
        // What was told yesterday is over: the hours say again.
        assert!(decide(false, told(false, 8), &week, monday_ten));
        assert!(!decide(false, told(true, 8), &week, monday_night));
    }

    #[test]
    fn speaking_to_ciel_again_ends_im_leaving_but_not_in_the_same_breath() {
        let start = Instant::now();
        let told = |here: bool| Some(Told { here, day: at(9, "00:00").date(), at: start });
        assert_eq!(after_speaking(told(false), start + Duration::from_secs(20)), told(false), "thanks, bye");
        assert_eq!(after_speaking(told(false), start + Duration::from_secs(600)), None);
        assert_eq!(after_speaking(told(true), start + Duration::from_secs(600)), told(true));
        assert_eq!(after_speaking(None, start), None);
    }

    /// At an unlocked desk, by hand: `cargo test -p ciel -- --ignored the_session`.
    #[test]
    #[ignore]
    fn the_session_reads_as_unlocked_while_someone_runs_this() {
        assert!(!crate::platform::session_locked());
        assert!(!crate::platform::presenting());
    }

    #[test]
    fn the_brain_says_here_or_away_and_nothing_else() {
        assert!(!for_brain(&serde_json::json!({})).1);
        assert!(!for_brain(&serde_json::json!({ "here": "yes" })).1);
    }
}
