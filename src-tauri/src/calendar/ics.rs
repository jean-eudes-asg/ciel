// Reading an iCalendar feed (RFC 5545), as the secret iCal links of Google,
// Outlook, iCloud or Fastmail give it: the events between two instants, the
// repeating ones expanded, in UTC.
//
// Times come three ways: in UTC ("…Z"); in a zone the feed describes in a
// VTIMEZONE, which every TZID it uses must have (Outlook's are Windows names,
// so the feed's own rules are the only ones that work for everyone); or
// floating, the computer's own time. Repeats are expanded on the wall clock of
// the event's zone, as the RFC says, then turned into instants: a weekly 10:00
// stays at 10:00 across a change of time.
//
// The repeat rules Google, Outlook and iCloud write are covered: DAILY,
// WEEKLY, MONTHLY and YEARLY, with INTERVAL, COUNT, UNTIL, BYDAY (and its
// ordinals), BYMONTHDAY, BYMONTH, BYSETPOS and WKST. A rule with anything else
// (BYYEARDAY, BYWEEKNO, BYHOUR…) keeps only its first occurrence.

use std::collections::{HashMap, HashSet};

use chrono::{DateTime, Datelike, Duration, NaiveDate, NaiveDateTime, NaiveTime, Weekday};

/// The most periods a rule is walked through: decades of a daily meeting.
const MAX_PERIODS: usize = 100_000;

// ── Content lines ─────────────────────────────────────────────────────────────

/// One property: `NAME;PARAM=value:value`.
#[derive(Debug, Clone, PartialEq)]
struct Prop {
    name: String,
    params: Vec<(String, String)>,
    value: String,
}

impl Prop {
    fn param(&self, name: &str) -> Option<&str> {
        self.params.iter().find(|(n, _)| n == name).map(|(_, v)| v.as_str())
    }
}

/// The feed's lines, the folded ones (a line starting with a space or a tab
/// goes on the one before) put back together.
fn unfold(text: &str) -> Vec<String> {
    let mut lines: Vec<String> = Vec::new();
    for raw in text.split('\n') {
        let line = raw.strip_suffix('\r').unwrap_or(raw);
        if let Some(rest) = line.strip_prefix(' ').or_else(|| line.strip_prefix('\t')) {
            if let Some(last) = lines.last_mut() {
                last.push_str(rest);
                continue;
            }
        }
        if !line.is_empty() {
            lines.push(line.to_string());
        }
    }
    lines
}

/// A content line, its parameters' quotes taken off; None when it isn't one.
fn parse_line(line: &str) -> Option<Prop> {
    let bytes = line.as_bytes();
    let mut i = 0;
    while i < bytes.len() && bytes[i] != b';' && bytes[i] != b':' {
        i += 1;
    }
    if i >= bytes.len() || i == 0 {
        return None;
    }
    let name = line[..i].to_ascii_uppercase();
    let mut params = Vec::new();
    while bytes[i] == b';' {
        i += 1;
        let start = i;
        while i < bytes.len() && !matches!(bytes[i], b'=' | b';' | b':') {
            i += 1;
        }
        if i >= bytes.len() {
            return None;
        }
        let param = line[start..i].to_ascii_uppercase();
        let mut values: Vec<&str> = Vec::new();
        if bytes[i] == b'=' {
            i += 1;
            loop {
                if i < bytes.len() && bytes[i] == b'"' {
                    let open = i + 1;
                    let close = open + line[open..].find('"')?;
                    values.push(&line[open..close]);
                    i = close + 1;
                } else {
                    let open = i;
                    while i < bytes.len() && !matches!(bytes[i], b',' | b';' | b':') {
                        i += 1;
                    }
                    values.push(&line[open..i]);
                }
                if i < bytes.len() && bytes[i] == b',' {
                    i += 1;
                    continue;
                }
                break;
            }
        }
        if i >= bytes.len() {
            return None;
        }
        params.push((param, values.join(",")));
    }
    (bytes[i] == b':').then(|| Prop { name, params, value: line[i + 1..].to_string() })
}

/// A TEXT value: `\n` is a new line, `\,` `\;` `\\` the character itself.
fn text(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    let mut chars = value.chars();
    while let Some(c) = chars.next() {
        if c == '\\' {
            match chars.next() {
                Some('n' | 'N') => out.push('\n'),
                Some(other) => out.push(other),
                None => {}
            }
        } else {
            out.push(c);
        }
    }
    out
}

// ── Values ────────────────────────────────────────────────────────────────────

/// A time as the feed writes it.
#[derive(Debug, Clone, PartialEq)]
enum When {
    /// A whole day (`VALUE=DATE`).
    Date(NaiveDate),
    /// An instant (`…Z`).
    Utc(NaiveDateTime),
    /// A wall-clock time in the zone named, or with none the computer's.
    Wall(NaiveDateTime, Option<String>),
}

fn parse_when(value: &str, tzid: Option<&str>, date_only: bool) -> Option<When> {
    let v = value.trim();
    if date_only || v.len() == 8 {
        return NaiveDate::parse_from_str(v, "%Y%m%d").ok().map(When::Date);
    }
    let (body, utc) = match v.strip_suffix('Z').or_else(|| v.strip_suffix('z')) {
        Some(body) => (body, true),
        None => (v, false),
    };
    let at = NaiveDateTime::parse_from_str(body, "%Y%m%dT%H%M%S").ok()?;
    Some(if utc { When::Utc(at) } else { When::Wall(at, tzid.map(|z| z.trim().to_string())) })
}

fn prop_when(p: &Prop) -> Option<When> {
    parse_when(&p.value, p.param("TZID"), p.param("VALUE") == Some("DATE"))
}

/// EXDATE and RDATE hold lists.
fn prop_whens(p: &Prop) -> Vec<When> {
    let date_only = p.param("VALUE") == Some("DATE");
    p.value.split(',').filter_map(|v| parse_when(v, p.param("TZID"), date_only)).collect()
}

/// `+0100`, `-0500`, `+053000` → seconds east of UTC.
fn parse_offset(value: &str) -> Option<i32> {
    let v = value.trim();
    let (sign, digits) = match v.as_bytes().first()? {
        b'+' => (1, &v[1..]),
        b'-' => (-1, &v[1..]),
        _ => return None,
    };
    if !(digits.len() == 4 || digits.len() == 6) || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let part = |r: std::ops::Range<usize>| digits.get(r).and_then(|s| s.parse::<i32>().ok()).unwrap_or(0);
    Some(sign * (part(0..2) * 3600 + part(2..4) * 60 + part(4..6)))
}

/// `PT1H30M`, `P1D`, `-PT15M`, `P2W`.
fn parse_duration(value: &str) -> Option<Duration> {
    let v = value.trim();
    let (negative, rest) = match v.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, v.strip_prefix('+').unwrap_or(v)),
    };
    let rest = rest.strip_prefix('P')?;
    let (mut secs, mut number, mut in_time, mut any) = (0i64, String::new(), false, false);
    for c in rest.chars() {
        match c {
            '0'..='9' => number.push(c),
            'T' => in_time = true,
            'W' | 'D' | 'H' | 'M' | 'S' => {
                let n: i64 = number.parse().ok()?;
                number.clear();
                any = true;
                secs += n * match (c, in_time) {
                    ('W', false) => 604_800,
                    ('D', false) => 86_400,
                    ('H', true) => 3_600,
                    ('M', true) => 60,
                    ('S', true) => 1,
                    _ => return None,
                };
            }
            _ => return None,
        }
    }
    (any && number.is_empty()).then(|| Duration::seconds(if negative { -secs } else { secs }))
}

// ── Repeat rules ──────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq)]
enum Freq {
    Daily,
    Weekly,
    Monthly,
    Yearly,
}

#[derive(Debug, Clone, PartialEq)]
struct Rule {
    freq: Freq,
    interval: u32,
    count: Option<u32>,
    until: Option<When>,
    /// Weekdays, with their ordinal in the month or the year (0: every one).
    by_day: Vec<(i32, Weekday)>,
    by_month_day: Vec<i32>,
    by_month: Vec<u32>,
    by_set_pos: Vec<i32>,
    week_start: Weekday,
}

fn weekday(code: &str) -> Option<Weekday> {
    Some(match code {
        "MO" => Weekday::Mon,
        "TU" => Weekday::Tue,
        "WE" => Weekday::Wed,
        "TH" => Weekday::Thu,
        "FR" => Weekday::Fri,
        "SA" => Weekday::Sat,
        "SU" => Weekday::Sun,
        _ => return None,
    })
}

/// `MO`, `2TU`, `-1FR`.
fn by_day(code: &str) -> Option<(i32, Weekday)> {
    let code = code.trim().to_ascii_uppercase();
    let (number, day) = code.split_at(code.len().checked_sub(2)?);
    let n = if number.is_empty() { 0 } else { number.parse::<i32>().ok().filter(|n| *n != 0 && n.abs() <= 53)? };
    Some((n, weekday(day)?))
}

/// None for a rule this reader doesn't cover: its event keeps its first occurrence.
fn parse_rule(value: &str) -> Option<Rule> {
    let mut rule = Rule {
        freq: Freq::Daily,
        interval: 1,
        count: None,
        until: None,
        by_day: Vec::new(),
        by_month_day: Vec::new(),
        by_month: Vec::new(),
        by_set_pos: Vec::new(),
        week_start: Weekday::Mon,
    };
    let mut freq = None;
    for part in value.trim().split(';').filter(|p| !p.is_empty()) {
        let (key, val) = part.split_once('=')?;
        let list = || val.split(',').filter(|s| !s.is_empty());
        match key.trim().to_ascii_uppercase().as_str() {
            "FREQ" => {
                freq = Some(match val.trim().to_ascii_uppercase().as_str() {
                    "DAILY" => Freq::Daily,
                    "WEEKLY" => Freq::Weekly,
                    "MONTHLY" => Freq::Monthly,
                    "YEARLY" => Freq::Yearly,
                    _ => return None,
                })
            }
            "INTERVAL" => rule.interval = val.trim().parse().ok().filter(|n| *n > 0)?,
            "COUNT" => rule.count = Some(val.trim().parse().ok()?),
            "UNTIL" => rule.until = Some(parse_when(val, None, false)?),
            "BYDAY" => {
                for d in list() {
                    rule.by_day.push(by_day(d)?);
                }
            }
            "BYMONTHDAY" => {
                for d in list() {
                    let n: i32 = d.trim().parse().ok().filter(|n: &i32| *n != 0 && n.abs() <= 31)?;
                    rule.by_month_day.push(n);
                }
            }
            "BYMONTH" => {
                for m in list() {
                    let n: u32 = m.trim().parse().ok().filter(|n| (1..=12).contains(n))?;
                    rule.by_month.push(n);
                }
            }
            "BYSETPOS" => {
                for p in list() {
                    let n: i32 = p.trim().parse().ok().filter(|n: &i32| *n != 0 && n.abs() <= 366)?;
                    rule.by_set_pos.push(n);
                }
            }
            "WKST" => rule.week_start = weekday(&val.trim().to_ascii_uppercase())?,
            _ => return None,
        }
    }
    rule.freq = freq?;
    Some(rule)
}

fn days_in_month(year: i32, month: u32) -> u32 {
    let next = if month == 12 { NaiveDate::from_ymd_opt(year + 1, 1, 1) } else { NaiveDate::from_ymd_opt(year, month + 1, 1) };
    next.and_then(|n| n.pred_opt()).map_or(28, |d| d.day())
}

/// Day `n` of a span of `len` days, counted from its end when negative.
fn resolve(n: i32, len: u32) -> Option<u32> {
    let d = if n > 0 { n } else { len as i32 + 1 + n };
    (1..=len as i32).contains(&d).then_some(d as u32)
}

/// Whether day `d` of a span of `len` days is the n-th of its weekday in it
/// (counted from the end when negative).
fn nth_in(d: u32, len: u32, n: i32) -> bool {
    if n > 0 {
        (d - 1) / 7 + 1 == n as u32
    } else {
        (len - d) / 7 + 1 == n.unsigned_abs()
    }
}

fn month_day_ok(rule: &Rule, date: NaiveDate) -> bool {
    rule.by_month_day.is_empty()
        || rule.by_month_day.iter().any(|&n| resolve(n, days_in_month(date.year(), date.month())) == Some(date.day()))
}

fn month_ok(rule: &Rule, date: NaiveDate) -> bool {
    rule.by_month.is_empty() || rule.by_month.contains(&date.month())
}

/// The days of one month a rule picks; `default_day` is DTSTART's, for a rule
/// that names no day (a month without it is skipped, as the RFC says).
fn month_days(rule: &Rule, year: i32, month: u32, default_day: u32) -> Vec<NaiveDate> {
    let last = days_in_month(year, month);
    let day = |d: u32| NaiveDate::from_ymd_opt(year, month, d);
    if rule.by_day.is_empty() {
        if rule.by_month_day.is_empty() {
            return day(default_day).into_iter().collect();
        }
        let mut days: Vec<NaiveDate> = rule.by_month_day.iter().filter_map(|&n| resolve(n, last)).filter_map(day).collect();
        days.sort();
        days.dedup();
        return days;
    }
    (1..=last)
        .filter_map(day)
        .filter(|&date| month_day_ok(rule, date))
        .filter(|&date| rule.by_day.iter().any(|&(n, wd)| date.weekday() == wd && (n == 0 || nth_in(date.day(), last, n))))
        .collect()
}

/// The `k`-th period of a rule from DTSTART's (k periods of INTERVAL): its
/// first day, and the days it picks in it, in order.
fn period(rule: &Rule, start: NaiveDate, k: i64) -> Option<(NaiveDate, Vec<NaiveDate>)> {
    let off = k.checked_mul(rule.interval as i64)?;
    match rule.freq {
        Freq::Daily => {
            let day = start.checked_add_signed(Duration::try_days(off)?)?;
            let keep = month_ok(rule, day)
                && month_day_ok(rule, day)
                && (rule.by_day.is_empty() || rule.by_day.iter().any(|&(_, wd)| wd == day.weekday()));
            Some((day, if keep { vec![day] } else { Vec::new() }))
        }
        Freq::Weekly => {
            let back = (start.weekday().num_days_from_monday() + 7 - rule.week_start.num_days_from_monday()) % 7;
            let first = start.checked_sub_signed(Duration::days(back as i64))?.checked_add_signed(Duration::try_weeks(off)?)?;
            let days = (0..7)
                .filter_map(|i| first.checked_add_signed(Duration::days(i)))
                .filter(|d| {
                    let picked = if rule.by_day.is_empty() {
                        d.weekday() == start.weekday()
                    } else {
                        rule.by_day.iter().any(|&(_, wd)| wd == d.weekday())
                    };
                    picked && month_ok(rule, *d)
                })
                .collect();
            Some((first, days))
        }
        Freq::Monthly => {
            let months = (start.year() as i64 * 12 + start.month0() as i64).checked_add(off)?;
            let (year, month) = (i32::try_from(months.div_euclid(12)).ok()?, (months.rem_euclid(12) + 1) as u32);
            let first = NaiveDate::from_ymd_opt(year, month, 1)?;
            let days = if rule.by_month.is_empty() || rule.by_month.contains(&month) {
                month_days(rule, year, month, start.day())
            } else {
                Vec::new()
            };
            Some((first, days))
        }
        Freq::Yearly => {
            let year = i32::try_from(start.year() as i64 + off).ok()?;
            let first = NaiveDate::from_ymd_opt(year, 1, 1)?;
            let mut days = Vec::new();
            if !rule.by_month.is_empty() {
                // Ordinals then count in the month.
                for &m in &rule.by_month {
                    days.extend(month_days(rule, year, m, start.day()));
                }
            } else if !rule.by_day.is_empty() {
                // Ordinals count in the year: 20MO is its 20th Monday.
                let len = if NaiveDate::from_ymd_opt(year, 2, 29).is_some() { 366 } else { 365 };
                for i in 0..len {
                    let Some(d) = first.checked_add_signed(Duration::days(i as i64)) else { break };
                    if month_day_ok(rule, d) && rule.by_day.iter().any(|&(n, wd)| d.weekday() == wd && (n == 0 || nth_in(i + 1, len, n))) {
                        days.push(d);
                    }
                }
            } else if !rule.by_month_day.is_empty() {
                for m in 1..=12 {
                    days.extend(month_days(rule, year, m, start.day()));
                }
            } else if let Some(d) = NaiveDate::from_ymd_opt(year, start.month(), start.day()) {
                days.push(d);
            }
            days.sort();
            days.dedup();
            Some((first, days))
        }
    }
}

/// BYSETPOS: the n-th days of a period's set (from its end when negative).
fn set_pos(rule: &Rule, days: Vec<NaiveDate>) -> Vec<NaiveDate> {
    if rule.by_set_pos.is_empty() {
        return days;
    }
    let len = days.len() as i32;
    let mut out: Vec<NaiveDate> = rule
        .by_set_pos
        .iter()
        .filter_map(|&p| {
            let i = if p > 0 { p - 1 } else { len + p };
            (0..len).contains(&i).then(|| days[i as usize])
        })
        .collect();
    out.sort();
    out.dedup();
    out
}

/// Whole periods of the rule's kind from `a` to `b` (0 when `b` is before).
fn periods_between(freq: Freq, a: NaiveDate, b: NaiveDate) -> i64 {
    let n = match freq {
        Freq::Daily => (b - a).num_days(),
        Freq::Weekly => (b - a).num_days().div_euclid(7),
        Freq::Monthly => (b.year() as i64 * 12 + b.month0() as i64) - (a.year() as i64 * 12 + a.month0() as i64),
        Freq::Yearly => b.year() as i64 - a.year() as i64,
    };
    n.max(0)
}

/// The wall-clock starts of a repeating event in [from, to], DTSTART (always
/// the first) included; `until` is UNTIL on the same wall clock.
fn expand(rule: &Rule, start: NaiveDateTime, until: Option<NaiveDateTime>, from: NaiveDateTime, to: NaiveDateTime) -> Vec<NaiveDateTime> {
    let mut out = Vec::new();
    if start > to {
        return out;
    }
    if start >= from && until.is_none_or(|u| start <= u) {
        out.push(start);
    }
    if rule.count.is_some_and(|c| c <= 1) {
        return out;
    }
    let mut counted = 1u32;
    // Without a count, the periods long before `from` can be skipped.
    let mut k = match rule.count {
        None => (periods_between(rule.freq, start.date(), from.date()) / rule.interval as i64 - 1).max(0),
        Some(_) => 0,
    };
    for _ in 0..MAX_PERIODS {
        let Some((first, days)) = period(rule, start.date(), k) else { break };
        if first > to.date() {
            break;
        }
        for day in set_pos(rule, days) {
            let at = day.and_time(start.time());
            if at <= start {
                continue;
            }
            if until.is_some_and(|u| at > u) || at > to {
                return out;
            }
            counted += 1;
            if at >= from {
                out.push(at);
            }
            if rule.count.is_some_and(|c| counted >= c) {
                return out;
            }
        }
        k += 1;
    }
    out
}

// ── Time zones ────────────────────────────────────────────────────────────────

/// A STANDARD or DAYLIGHT part of a VTIMEZONE: from its onsets on, `to` is in force.
#[derive(Debug, Clone)]
struct Observance {
    /// The first onset, on the clock before it (TZOFFSETFROM).
    start: NaiveDateTime,
    from: i32,
    to: i32,
    rule: Option<Rule>,
    dates: Vec<NaiveDateTime>,
}

#[derive(Debug, Default)]
struct ObservanceParts {
    start: Option<NaiveDateTime>,
    from: Option<i32>,
    to: Option<i32>,
    rule: Option<Rule>,
    dates: Vec<NaiveDateTime>,
}

impl ObservanceParts {
    fn take(&mut self, p: &Prop) {
        let wall = |w: When| match w {
            When::Wall(at, _) | When::Utc(at) => Some(at),
            When::Date(d) => Some(d.and_time(NaiveTime::MIN)),
        };
        match p.name.as_str() {
            "DTSTART" => self.start = prop_when(p).and_then(wall),
            "TZOFFSETFROM" => self.from = parse_offset(&p.value),
            "TZOFFSETTO" => self.to = parse_offset(&p.value),
            "RRULE" => self.rule = parse_rule(&p.value),
            "RDATE" => self.dates.extend(prop_whens(p).into_iter().filter_map(wall)),
            _ => {}
        }
    }

    fn build(self) -> Option<Observance> {
        let to = self.to?;
        Some(Observance { start: self.start?, from: self.from.unwrap_or(to), to, rule: self.rule, dates: self.dates })
    }
}

impl Observance {
    /// Its last onset at or before `wall`, if it has begun.
    fn last_onset(&self, wall: NaiveDateTime) -> Option<NaiveDateTime> {
        if self.start > wall {
            return None;
        }
        let mut best = self.start;
        for &d in &self.dates {
            if d <= wall && d > best {
                best = d;
            }
        }
        if let Some(rule) = &self.rule {
            let until = rule.until.as_ref().map(|u| match u {
                When::Utc(at) => *at + Duration::seconds(self.from as i64),
                When::Wall(at, _) => *at,
                When::Date(d) => d.and_time(NaiveTime::MIN) + Duration::days(1),
            });
            let end = until.map_or(wall, |u| u.min(wall));
            let from = end.checked_sub_signed(Duration::days(400)).unwrap_or(end).max(self.start);
            if let Some(&last) = expand(rule, self.start, until, from, end).last() {
                best = best.max(last);
            }
        }
        Some(best)
    }
}

#[derive(Debug, Clone, Default)]
struct Zone {
    observances: Vec<Observance>,
}

impl Zone {
    /// Seconds east of UTC at a wall-clock time of the zone. In the hour a
    /// change of time repeats, the first one.
    fn offset_at(&self, wall: NaiveDateTime) -> i32 {
        let mut best: Option<(NaiveDateTime, i32)> = None;
        for o in &self.observances {
            if let Some(onset) = o.last_onset(wall) {
                if best.is_none_or(|(b, _)| onset > b) {
                    best = Some((onset, o.to));
                }
            }
        }
        best.map(|(_, offset)| offset)
            .or_else(|| self.observances.iter().min_by_key(|o| o.start).map(|o| o.from))
            .unwrap_or(0)
    }
}

/// TZIDs that are UTC whether or not the feed says so.
fn is_utc_name(tzid: &str) -> bool {
    matches!(tzid, "UTC" | "Etc/UTC" | "GMT" | "Etc/GMT" | "Z" | "Universal" | "Zulu")
}

// ── Events ────────────────────────────────────────────────────────────────────

/// Someone invited, as the feed names them.
#[derive(Debug, Clone, PartialEq)]
pub struct Attendee {
    pub name: String,
    pub email: Option<String>,
    pub declined: bool,
    /// A meeting room or a projector, invited like a person.
    pub resource: bool,
}

fn attendee(p: &Prop) -> Attendee {
    let value = p.value.trim();
    let address = if value.len() >= 7 && value[..7].eq_ignore_ascii_case("mailto:") { &value[7..] } else { value };
    let email = Some(address.trim().to_string()).filter(|e| e.contains('@'));
    let name = p
        .param("CN")
        .map(|c| c.trim().to_string())
        .filter(|c| !c.is_empty())
        .or_else(|| email.as_deref().and_then(|e| e.split('@').next()).map(String::from))
        .unwrap_or_default();
    Attendee {
        name,
        email,
        declined: p.param("PARTSTAT").is_some_and(|s| s.eq_ignore_ascii_case("DECLINED")),
        resource: p.param("CUTYPE").is_some_and(|c| c.eq_ignore_ascii_case("RESOURCE") || c.eq_ignore_ascii_case("ROOM")),
    }
}

#[derive(Debug, Clone, Default)]
struct Event {
    uid: String,
    title: String,
    location: Option<String>,
    description: Option<String>,
    /// URL, and the video links Google and Outlook add as properties of their own.
    links: Vec<String>,
    start: Option<When>,
    end: Option<When>,
    duration: Option<Duration>,
    repeats: bool,
    rule: Option<Rule>,
    exdates: Vec<When>,
    rdates: Vec<When>,
    recurrence_id: Option<When>,
    cancelled: bool,
    attendees: Vec<Attendee>,
}

impl Event {
    fn take(&mut self, p: &Prop) {
        let some_text = |v: &str| Some(text(v).trim().to_string()).filter(|s| !s.is_empty());
        match p.name.as_str() {
            "UID" => self.uid = p.value.trim().to_string(),
            "SUMMARY" => self.title = text(&p.value).trim().to_string(),
            "LOCATION" => self.location = some_text(&p.value),
            "DESCRIPTION" => self.description = some_text(&p.value),
            "URL" | "X-GOOGLE-CONFERENCE" | "X-MICROSOFT-SKYPETEAMSMEETINGURL" | "X-MICROSOFT-ONLINEMEETINGCONFLINK" => {
                self.links.extend(some_text(&p.value))
            }
            "DTSTART" => self.start = prop_when(p),
            "DTEND" => self.end = prop_when(p),
            "DURATION" => self.duration = parse_duration(&p.value),
            "RRULE" => {
                self.repeats = true;
                self.rule = parse_rule(&p.value);
            }
            "EXDATE" => self.exdates.extend(prop_whens(p)),
            "RDATE" if p.param("VALUE") != Some("PERIOD") => self.rdates.extend(prop_whens(p)),
            "RECURRENCE-ID" => self.recurrence_id = prop_when(p),
            "STATUS" => self.cancelled = p.value.trim().eq_ignore_ascii_case("CANCELLED"),
            "ATTENDEE" => self.attendees.push(attendee(p)),
            _ => {}
        }
    }
}

/// One occurrence of an event, ready for the island.
#[derive(Debug, Clone, PartialEq)]
pub struct Occurrence {
    pub uid: String,
    pub title: String,
    /// Unix ms.
    pub start: i64,
    pub end: i64,
    pub all_day: bool,
    pub location: Option<String>,
    pub description: Option<String>,
    /// Where it is joined: a Meet, Teams, Zoom… address.
    pub join: Option<String>,
    /// Who else is invited: not the calendar's owner, not the rooms.
    pub attendees: Vec<Attendee>,
    /// The calendar's owner said no.
    pub declined: bool,
}

/// A calendar, read.
#[derive(Debug, Default)]
pub struct Feed {
    /// X-WR-CALNAME. Google names a person's own calendar with their address.
    pub name: Option<String>,
    zones: HashMap<String, Zone>,
    events: Vec<Event>,
}

impl Feed {
    /// None when the text isn't a calendar.
    pub fn parse(text_in: &str) -> Option<Feed> {
        let mut feed = Feed::default();
        let mut stack: Vec<String> = Vec::new();
        let mut event: Option<Event> = None;
        let mut zone: Option<(String, Zone)> = None;
        let mut observance: Option<ObservanceParts> = None;
        let mut calendar = false;
        for line in unfold(text_in.trim_start_matches('\u{feff}')) {
            let Some(p) = parse_line(&line) else { continue };
            match p.name.as_str() {
                "BEGIN" => {
                    let component = p.value.trim().to_ascii_uppercase();
                    match (stack.last().map(String::as_str), component.as_str()) {
                        (None, "VCALENDAR") => calendar = true,
                        (Some("VCALENDAR"), "VEVENT") => event = Some(Event::default()),
                        (Some("VCALENDAR"), "VTIMEZONE") => zone = Some((String::new(), Zone::default())),
                        (Some("VTIMEZONE"), "STANDARD" | "DAYLIGHT") => observance = Some(ObservanceParts::default()),
                        _ => {}
                    }
                    stack.push(component);
                }
                "END" => match stack.pop().as_deref() {
                    Some("VEVENT") => feed.events.extend(event.take()),
                    Some("VTIMEZONE") => {
                        if let Some((id, z)) = zone.take().filter(|(id, _)| !id.is_empty()) {
                            feed.zones.insert(id, z);
                        }
                    }
                    Some("STANDARD" | "DAYLIGHT") => {
                        if let (Some(parts), Some((_, z))) = (observance.take(), zone.as_mut()) {
                            z.observances.extend(parts.build());
                        }
                    }
                    _ => {}
                },
                _ => match stack.last().map(String::as_str) {
                    Some("VCALENDAR") if p.name == "X-WR-CALNAME" => feed.name = Some(text(&p.value).trim().to_string()),
                    Some("VEVENT") => {
                        if let Some(e) = event.as_mut() {
                            e.take(&p);
                        }
                    }
                    Some("VTIMEZONE") if p.name == "TZID" => {
                        if let Some((id, _)) = zone.as_mut() {
                            *id = p.value.trim().to_string();
                        }
                    }
                    Some("STANDARD" | "DAYLIGHT") => {
                        if let Some(parts) = observance.as_mut() {
                            parts.take(&p);
                        }
                    }
                    _ => {}
                },
            }
        }
        calendar.then_some(feed)
    }

    /// A wall-clock time turned into an instant (Unix ms), through its zone:
    /// the feed's own, UTC, or else the computer's (`local`).
    fn wall_instant(&self, at: NaiveDateTime, tzid: Option<&str>, local: &dyn Fn(NaiveDateTime) -> i64) -> i64 {
        match tzid {
            Some(z) if is_utc_name(z) => at.and_utc().timestamp_millis(),
            Some(z) if self.zones.contains_key(z) => {
                let offset = self.zones[z].offset_at(at);
                (at - Duration::seconds(offset as i64)).and_utc().timestamp_millis()
            }
            _ => local(at),
        }
    }

    fn instant(&self, w: &When, local: &dyn Fn(NaiveDateTime) -> i64) -> i64 {
        match w {
            When::Utc(at) => at.and_utc().timestamp_millis(),
            When::Wall(at, tzid) => self.wall_instant(*at, tzid.as_deref(), local),
            When::Date(d) => local(d.and_time(NaiveTime::MIN)),
        }
    }

    /// The occurrences that overlap [from, to) (Unix ms), in order of start.
    /// `local` turns a wall-clock time of the computer into an instant.
    pub fn between(&self, from: i64, to: i64, local: &dyn Fn(NaiveDateTime) -> i64) -> Vec<Occurrence> {
        let me = self.name.as_deref().filter(|n| n.contains('@')).map(str::to_ascii_lowercase);
        // Occurrences moved or cancelled one by one, by their event and first start.
        let moved: HashSet<(&str, i64)> = self
            .events
            .iter()
            .filter_map(|e| Some((e.uid.as_str(), self.instant(e.recurrence_id.as_ref()?, local))))
            .collect();
        let mut out = Vec::new();
        for e in self.events.iter().filter(|e| !e.cancelled) {
            let Some(start) = &e.start else { continue };
            let first = self.instant(start, local);
            let all_day = matches!(start, When::Date(_));
            let days = match (start, &e.end) {
                (When::Date(a), Some(When::Date(b))) => (*b - *a).num_days().max(1),
                _ => e.duration.map_or(1, |d| d.num_days().max(1)),
            };
            let length = match (&e.end, e.duration) {
                (Some(end), _) => (self.instant(end, local) - first).max(0),
                (None, Some(d)) => d.num_milliseconds().max(0),
                (None, None) => 0,
            };
            let mut starts: Vec<i64> = if e.repeats && e.recurrence_id.is_none() {
                self.repeats(e, start, length, from, to, local)
            } else {
                vec![first]
            };
            starts.sort_unstable();
            starts.dedup();

            let mine = |a: &Attendee| me.as_deref().is_some_and(|m| a.email.as_deref().is_some_and(|e| e.eq_ignore_ascii_case(m)));
            for s in starts {
                if e.recurrence_id.is_none() && e.repeats && moved.contains(&(e.uid.as_str(), s)) {
                    continue;
                }
                let end = if all_day { local_days_later(s, days, local) } else {
                    s + length
                };
                if !(s < to && end.max(s + 1) > from) {
                    continue;
                }
                let links: Vec<&str> = e.links.iter().map(String::as_str).chain(e.location.as_deref()).chain(e.description.as_deref()).collect();
                out.push(Occurrence {
                    uid: e.uid.clone(),
                    title: e.title.clone(),
                    start: s,
                    end,
                    all_day,
                    location: e.location.clone(),
                    description: e.description.clone(),
                    join: join_link(&links),
                    attendees: e.attendees.iter().filter(|a| !a.resource && !mine(a)).cloned().collect(),
                    declined: e.attendees.iter().any(|a| mine(a) && a.declined),
                });
            }
        }
        out.sort_by(|a, b| a.start.cmp(&b.start).then_with(|| a.title.cmp(&b.title)));
        out
    }

    /// The starts (Unix ms) of a repeating event that may overlap [from, to).
    fn repeats(&self, e: &Event, start: &When, length: i64, from: i64, to: i64, local: &dyn Fn(NaiveDateTime) -> i64) -> Vec<i64> {
        let (wall, tzid): (NaiveDateTime, Option<&str>) = match start {
            When::Utc(at) => (*at, Some("UTC")),
            When::Wall(at, tzid) => (*at, tzid.as_deref()),
            When::Date(d) => (d.and_time(NaiveTime::MIN), None),
        };
        let to_instant = |at: NaiveDateTime| match start {
            When::Date(_) => local(at),
            _ => self.wall_instant(at, tzid, local),
        };
        // The window on the event's wall clock: a day either side covers any
        // offset, and an occurrence that began before `from` may still run.
        let naive = |ms: i64| DateTime::from_timestamp_millis(ms).map(|d| d.naive_utc()).unwrap_or_default();
        let window_from = naive(from) - Duration::days(1) - Duration::milliseconds(length.max(86_400_000));
        let window_to = naive(to) + Duration::days(1);
        let mut starts: Vec<i64> = match &e.rule {
            Some(rule) => {
                // UNTIL on the same wall clock: an instant moved by the zone's offset then.
                let until = rule.until.as_ref().map(|u| match u {
                    When::Utc(at) => *at + Duration::milliseconds(at.and_utc().timestamp_millis() - to_instant(*at)),
                    When::Wall(at, _) => *at,
                    When::Date(d) => d.and_hms_opt(23, 59, 59).unwrap_or_default(),
                });
                expand(rule, wall, until, window_from, window_to).into_iter().map(to_instant).collect()
            }
            // A rule this reader doesn't know: the first occurrence alone.
            None => vec![to_instant(wall)],
        };
        starts.extend(e.rdates.iter().map(|w| self.instant(w, local)));
        let excluded: HashSet<i64> = e.exdates.iter().map(|w| self.instant(w, local)).collect();
        starts.retain(|s| !excluded.contains(s));
        starts
    }
}

/// The local midnight `days` after the day of `start` (an all-day event's end).
fn local_days_later(start: i64, days: i64, local: &dyn Fn(NaiveDateTime) -> i64) -> i64 {
    // The day `start` is the local midnight of: found back by trying the
    // dates around its UTC one, which differ by the offset at most.
    let utc_day = DateTime::from_timestamp_millis(start).map(|d| d.date_naive()).unwrap_or_default();
    for shift in [0i64, 1, -1] {
        let Some(day) = utc_day.checked_add_signed(Duration::days(shift)) else { continue };
        if local(day.and_time(NaiveTime::MIN)) == start {
            if let Some(end_day) = day.checked_add_signed(Duration::days(days)) {
                return local(end_day.and_time(NaiveTime::MIN));
            }
        }
    }
    start + days * 86_400_000
}

// ── Meetings ──────────────────────────────────────────────────────────────────

/// The video services a meeting is joined on, and what of an address says it
/// is a meeting room (a description carries other links of the same sites:
/// phone numbers, help pages). The host may be a subdomain ("us02web.zoom.us").
const MEETING_HOSTS: &[(&str, &[&str])] = &[
    ("meet.google.com", &["/"]),
    ("zoom.us", &["/j/", "/my/", "/w/"]),
    ("zoomgov.com", &["/j/", "/my/"]),
    ("teams.microsoft.com", &["/l/meetup-join/", "/meet/"]),
    ("teams.live.com", &["/meet/"]),
    ("webex.com", &["/meet/", "/join/", "j.php"]),
    ("whereby.com", &["/"]),
    ("meet.jit.si", &["/"]),
    ("app.cal.com", &["/video/"]),
    ("gotomeeting.com", &["/join/"]),
    ("meet.goto.com", &["/"]),
    ("join.skype.com", &["/"]),
    ("chime.aws", &["/"]),
];

/// The https addresses in a text.
fn urls(text: &str) -> impl Iterator<Item = &str> {
    text.match_indices("https://").map(move |(i, _)| {
        let rest = &text[i..];
        let end = rest
            .find(|c: char| c.is_whitespace() || matches!(c, '<' | '>' | '"' | '\'' | '(' | ')' | '[' | ']' | '{' | '}' | '|' | '\\' | '^' | '`'))
            .unwrap_or(rest.len());
        rest[..end].trim_end_matches(['.', ',', ';', ':', '!', '?'])
    })
}

fn is_meeting_url(url: &str) -> bool {
    let after = &url["https://".len()..];
    let split = after.find(['/', '?', '#']).unwrap_or(after.len());
    let (host, path) = after.split_at(split);
    if host.is_empty() || host.contains('@') {
        return false;
    }
    let host = host.split(':').next().unwrap_or(host).to_ascii_lowercase();
    MEETING_HOSTS.iter().any(|(h, marks)| {
        (host == *h || host.strip_suffix(h).is_some_and(|sub| sub.ends_with('.')))
            && path.len() > 1
            && marks.iter().any(|m| path.contains(m))
    })
}

/// The first meeting address in these texts, in their order: a video
/// service's room, never any other link.
pub fn join_link(texts: &[&str]) -> Option<String> {
    texts.iter().flat_map(|t| urls(t)).find(|u| is_meeting_url(u)).map(String::from)
}

/// Text for a description written in HTML (Google keeps the formatting of
/// its editor): the tags gone, the lines kept.
fn plain(html: &str) -> String {
    if !(html.contains('<') && html.contains('>')) {
        return html.to_string();
    }
    let mut out = String::with_capacity(html.len());
    let mut rest = html;
    while let Some(open) = rest.find('<') {
        out.push_str(&rest[..open]);
        let Some(close) = rest[open..].find('>') else {
            out.push_str(&rest[open..]);
            rest = "";
            break;
        };
        let tag = rest[open + 1..open + close].trim_start_matches('/').to_ascii_lowercase();
        let name = tag.split(|c: char| !c.is_ascii_alphanumeric()).next().unwrap_or("");
        if matches!(name, "br" | "p" | "div" | "li" | "tr" | "h1" | "h2" | "h3" | "ul" | "ol") {
            out.push('\n');
        }
        rest = &rest[open + close + 1..];
    }
    out.push_str(rest);
    out.replace("&nbsp;", " ")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&amp;", "&")
}

/// What a meeting's description says about it, for the notes' summary:
/// without the joining instructions Google Meet, Teams or Zoom add, at most
/// 2,000 characters. None when nothing is left.
pub fn agenda(description: &str) -> Option<String> {
    let text = plain(description);
    let mut kept: Vec<&str> = Vec::new();
    for line in text.lines() {
        let t = line.trim();
        // Google's "-::~:~::~:" and Teams's line of underscores open their part.
        if t.starts_with("-::~") || (t.chars().count() >= 20 && t.chars().all(|c| c == '_' || c == '─')) {
            break;
        }
        if urls(t).any(is_meeting_url) {
            continue;
        }
        kept.push(line.trim_end());
    }
    let mut out = kept.join("\n");
    while out.contains("\n\n\n") {
        out = out.replace("\n\n\n", "\n\n");
    }
    let out = out.trim();
    if out.is_empty() {
        return None;
    }
    Some(match out.char_indices().nth(2000) {
        Some((cut, _)) => format!("{}…", &out[..cut]),
        None => out.to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The computer's clock in these tests: Paris, as a fixed +01:00 (the
    /// tests that care about changes of time name their zone).
    fn paris_winter(at: NaiveDateTime) -> i64 {
        (at - Duration::hours(1)).and_utc().timestamp_millis()
    }

    fn ms(s: &str) -> i64 {
        DateTime::parse_from_rfc3339(s).unwrap().timestamp_millis()
    }

    fn feed(events: &str) -> Feed {
        Feed::parse(&format!("BEGIN:VCALENDAR\r\nVERSION:2.0\r\n{PARIS}{events}END:VCALENDAR\r\n")).expect("a calendar")
    }

    /// Google's VTIMEZONE for Paris.
    const PARIS: &str = "BEGIN:VTIMEZONE\r\nTZID:Europe/Paris\r\nX-LIC-LOCATION:Europe/Paris\r\n\
BEGIN:DAYLIGHT\r\nTZOFFSETFROM:+0100\r\nTZOFFSETTO:+0200\r\nTZNAME:CEST\r\nDTSTART:19700329T020000\r\n\
RRULE:FREQ=YEARLY;BYMONTH=3;BYDAY=-1SU\r\nEND:DAYLIGHT\r\n\
BEGIN:STANDARD\r\nTZOFFSETFROM:+0200\r\nTZOFFSETTO:+0100\r\nTZNAME:CET\r\nDTSTART:19701025T030000\r\n\
RRULE:FREQ=YEARLY;BYMONTH=10;BYDAY=-1SU\r\nEND:STANDARD\r\nEND:VTIMEZONE\r\n";

    /// Outlook's, under its Windows name and with a 1601 start.
    const OUTLOOK_NEW_YORK: &str = "BEGIN:VTIMEZONE\r\nTZID:Eastern Standard Time\r\n\
BEGIN:STANDARD\r\nDTSTART:16010101T020000\r\nTZOFFSETFROM:-0400\r\nTZOFFSETTO:-0500\r\n\
RRULE:FREQ=YEARLY;INTERVAL=1;BYDAY=1SU;BYMONTH=11\r\nEND:STANDARD\r\n\
BEGIN:DAYLIGHT\r\nDTSTART:16010101T020000\r\nTZOFFSETFROM:-0500\r\nTZOFFSETTO:-0400\r\n\
RRULE:FREQ=YEARLY;INTERVAL=1;BYDAY=2SU;BYMONTH=3\r\nEND:DAYLIGHT\r\nEND:VTIMEZONE\r\n";

    fn event(body: &str) -> String {
        format!("BEGIN:VEVENT\r\n{body}END:VEVENT\r\n")
    }

    fn starts(f: &Feed, from: &str, to: &str) -> Vec<i64> {
        f.between(ms(from), ms(to), &paris_winter).iter().map(|o| o.start).collect()
    }

    // ── Lines and values ──

    #[test]
    fn folded_lines_and_quoted_parameters_are_read() {
        let lines = unfold("SUMMARY:Weekly\r\n  sync\r\nATTENDEE;CN=\"Dupont, Marie\";PARTSTAT=ACCEPTED:mailto:m@x.fr\r\n");
        assert_eq!(lines[0], "SUMMARY:Weekly sync");
        let p = parse_line(&lines[1]).unwrap();
        assert_eq!(p.name, "ATTENDEE");
        assert_eq!(p.param("CN"), Some("Dupont, Marie"));
        assert_eq!(p.param("PARTSTAT"), Some("ACCEPTED"));
        assert_eq!(p.value, "mailto:m@x.fr");
        assert_eq!(parse_line("DESCRIPTION:a:b").unwrap().value, "a:b");
        assert!(parse_line("no colon here").is_none());
        assert_eq!(text("Room 4\\, floor 2\\nBring\\; slides\\\\"), "Room 4, floor 2\nBring; slides\\");
    }

    #[test]
    fn offsets_and_durations_are_read() {
        assert_eq!(parse_offset("+0100"), Some(3600));
        assert_eq!(parse_offset("-0430"), Some(-(4 * 3600 + 30 * 60)));
        assert_eq!(parse_offset("+053000"), Some(5 * 3600 + 30 * 60));
        assert_eq!(parse_offset("0100"), None);
        assert_eq!(parse_duration("PT1H30M"), Some(Duration::minutes(90)));
        assert_eq!(parse_duration("P1D"), Some(Duration::days(1)));
        assert_eq!(parse_duration("P2W"), Some(Duration::weeks(2)));
        assert_eq!(parse_duration("-PT15M"), Some(Duration::minutes(-15)));
        assert_eq!(parse_duration("P1H"), None, "hours belong after T");
    }

    #[test]
    fn what_isnt_a_calendar_is_refused() {
        assert!(Feed::parse("<html>Sign in</html>").is_none());
        assert!(Feed::parse("").is_none());
        assert!(Feed::parse("\u{feff}BEGIN:VCALENDAR\nEND:VCALENDAR\n").is_some());
    }

    // ── Time zones ──

    #[test]
    fn times_in_utc_in_a_described_zone_and_floating() {
        let f = feed(&[
            event("UID:a\r\nSUMMARY:UTC\r\nDTSTART:20261020T130000Z\r\nDTEND:20261020T133000Z\r\n"),
            event("UID:b\r\nSUMMARY:Paris\r\nDTSTART;TZID=Europe/Paris:20261020T150000\r\nDTEND;TZID=Europe/Paris:20261020T160000\r\n"),
            event("UID:c\r\nSUMMARY:Floating\r\nDTSTART:20261020T170000\r\nDURATION:PT45M\r\n"),
        ]
        .concat());
        let all = f.between(ms("2026-10-20T00:00:00Z"), ms("2026-10-21T00:00:00Z"), &paris_winter);
        let got: Vec<(&str, i64, i64)> = all.iter().map(|o| (o.title.as_str(), o.start, o.end)).collect();
        assert_eq!(got, vec![
            // Summer time in Paris until the 25th: +02:00. The same start as
            // the next: by title.
            ("Paris", ms("2026-10-20T13:00:00Z"), ms("2026-10-20T14:00:00Z")),
            ("UTC", ms("2026-10-20T13:00:00Z"), ms("2026-10-20T13:30:00Z")),
            ("Floating", ms("2026-10-20T16:00:00Z"), ms("2026-10-20T16:45:00Z")),
        ]);
    }

    #[test]
    fn a_weekly_meeting_keeps_its_wall_clock_across_the_change_of_time() {
        // Every Tuesday at 10:00 in Paris; the clocks go back on Sunday 25 October 2026.
        let f = feed(&event("UID:w\r\nSUMMARY:Standup\r\nDTSTART;TZID=Europe/Paris:20260901T100000\r\nDTEND;TZID=Europe/Paris:20260901T101500\r\nRRULE:FREQ=WEEKLY;BYDAY=TU\r\n"));
        assert_eq!(starts(&f, "2026-10-19T00:00:00Z", "2026-11-04T00:00:00Z"), vec![
            ms("2026-10-20T08:00:00Z"),
            ms("2026-10-27T09:00:00Z"),
            ms("2026-11-03T09:00:00Z"),
        ]);
    }

    #[test]
    fn the_hour_a_change_of_time_repeats_reads_as_its_first() {
        let f = feed("");
        let paris = &f.zones["Europe/Paris"];
        let at = |s: &str| NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M").unwrap();
        assert_eq!(paris.offset_at(at("2026-10-25 01:30")), 7200);
        assert_eq!(paris.offset_at(at("2026-10-25 02:30")), 7200);
        assert_eq!(paris.offset_at(at("2026-10-25 03:00")), 3600);
        assert_eq!(paris.offset_at(at("2026-03-29 01:59")), 3600);
        assert_eq!(paris.offset_at(at("2026-03-29 03:00")), 7200);
        assert_eq!(paris.offset_at(at("1960-01-01 00:00")), 3600, "before the zone's first onset");
    }

    #[test]
    fn outlooks_windows_zone_names_work_through_the_feeds_own_rules() {
        let f = Feed::parse(&format!(
            "BEGIN:VCALENDAR\r\n{OUTLOOK_NEW_YORK}{}END:VCALENDAR\r\n",
            event("UID:o\r\nSUMMARY:Review\r\nDTSTART;TZID=Eastern Standard Time:20261030T090000\r\nDTEND;TZID=Eastern Standard Time:20261030T100000\r\nRRULE:FREQ=WEEKLY;BYDAY=FR;COUNT=3\r\n"),
        ))
        .unwrap();
        // New York leaves summer time on 1 November 2026.
        assert_eq!(starts(&f, "2026-10-01T00:00:00Z", "2026-12-01T00:00:00Z"), vec![
            ms("2026-10-30T13:00:00Z"),
            ms("2026-11-06T14:00:00Z"),
            ms("2026-11-13T14:00:00Z"),
        ]);
    }

    #[test]
    fn a_zone_the_feed_doesnt_describe_is_the_computers() {
        let f = feed(&event("UID:z\r\nSUMMARY:X\r\nDTSTART;TZID=Somewhere/Else:20261020T120000\r\n"));
        assert_eq!(starts(&f, "2026-10-20T00:00:00Z", "2026-10-21T00:00:00Z"), vec![ms("2026-10-20T11:00:00Z")]);
        let f = feed(&event("UID:u\r\nSUMMARY:X\r\nDTSTART;TZID=UTC:20261020T120000\r\n"));
        assert_eq!(starts(&f, "2026-10-20T00:00:00Z", "2026-10-21T00:00:00Z"), vec![ms("2026-10-20T12:00:00Z")]);
    }

    // ── Repeats ──

    fn days_of(rule: &str, start: &str, from: &str, to: &str) -> Vec<String> {
        let rule = parse_rule(rule).expect("a rule");
        let at = |s: &str| NaiveDateTime::parse_from_str(&format!("{s} 09:00"), "%Y-%m-%d %H:%M").unwrap();
        let until = rule.until.as_ref().map(|u| match u {
            When::Utc(at) | When::Wall(at, _) => *at,
            When::Date(d) => d.and_hms_opt(23, 59, 59).unwrap(),
        });
        expand(&rule, at(start), until, at(from), at(to)).iter().map(|d| d.format("%Y-%m-%d").to_string()).collect()
    }

    #[test]
    fn daily_weekly_and_their_limits() {
        assert_eq!(days_of("FREQ=DAILY;COUNT=3", "2026-10-20", "2026-01-01", "2027-01-01"), ["2026-10-20", "2026-10-21", "2026-10-22"]);
        assert_eq!(days_of("FREQ=DAILY;INTERVAL=2;UNTIL=20261026T000000Z", "2026-10-20", "2026-01-01", "2027-01-01"), ["2026-10-20", "2026-10-22", "2026-10-24"]);
        assert_eq!(
            days_of("FREQ=WEEKLY;BYDAY=MO,WE,FR", "2026-10-19", "2026-10-21", "2026-10-28"),
            ["2026-10-21", "2026-10-23", "2026-10-26", "2026-10-28"]
        );
        // Every other week from a Wednesday, on Monday and Wednesday, weeks starting on Sunday.
        assert_eq!(
            days_of("FREQ=WEEKLY;INTERVAL=2;BYDAY=MO,WE;WKST=SU", "2026-10-21", "2026-10-01", "2026-11-20"),
            ["2026-10-21", "2026-11-02", "2026-11-04", "2026-11-16", "2026-11-18"]
        );
        // Weekdays only, for years: the start is far behind the window.
        assert_eq!(
            days_of("FREQ=WEEKLY;BYDAY=MO,TU,WE,TH,FR", "2015-01-05", "2026-10-23", "2026-10-27"),
            ["2026-10-23", "2026-10-26", "2026-10-27"]
        );
        // A count is counted from the start, not from the window.
        assert_eq!(days_of("FREQ=WEEKLY;COUNT=10", "2026-09-01", "2026-11-01", "2027-01-01"), ["2026-11-03"]);
    }

    #[test]
    fn monthly_by_day_by_weekday_and_by_position() {
        assert_eq!(days_of("FREQ=MONTHLY;BYDAY=2TU", "2026-10-13", "2026-10-01", "2027-01-01"), ["2026-10-13", "2026-11-10", "2026-12-08"]);
        assert_eq!(days_of("FREQ=MONTHLY;BYDAY=-1FR", "2026-10-30", "2026-10-01", "2027-01-01"), ["2026-10-30", "2026-11-27", "2026-12-25"]);
        assert_eq!(days_of("FREQ=MONTHLY;BYMONTHDAY=-1", "2026-10-31", "2026-10-01", "2027-01-01"), ["2026-10-31", "2026-11-30", "2026-12-31"]);
        // The 31st: months without one are skipped.
        assert_eq!(days_of("FREQ=MONTHLY", "2026-08-31", "2026-08-01", "2027-01-01"), ["2026-08-31", "2026-10-31", "2026-12-31"]);
        // Outlook's "last weekday of the month".
        assert_eq!(
            days_of("FREQ=MONTHLY;BYDAY=MO,TU,WE,TH,FR;BYSETPOS=-1", "2026-10-30", "2026-10-01", "2027-01-01"),
            ["2026-10-30", "2026-11-30", "2026-12-31"]
        );
        assert_eq!(days_of("FREQ=MONTHLY;INTERVAL=3;BYMONTHDAY=1", "2026-01-01", "2026-01-01", "2026-12-31"), ["2026-01-01", "2026-04-01", "2026-07-01", "2026-10-01"]);
    }

    #[test]
    fn yearly_rules() {
        assert_eq!(days_of("FREQ=YEARLY", "2024-02-29", "2024-01-01", "2029-01-01"), ["2024-02-29", "2028-02-29"]);
        assert_eq!(days_of("FREQ=YEARLY;BYMONTH=11;BYDAY=4TH", "2026-11-26", "2026-01-01", "2028-12-31"), ["2026-11-26", "2027-11-25", "2028-11-23"]);
        assert_eq!(days_of("FREQ=YEARLY;BYDAY=20MO", "2026-05-18", "2026-01-01", "2027-12-31"), ["2026-05-18", "2027-05-17"]);
        assert_eq!(days_of("FREQ=YEARLY;BYMONTH=1,7;BYMONTHDAY=15", "2026-01-15", "2026-01-01", "2027-12-31"), ["2026-01-15", "2026-07-15", "2027-01-15", "2027-07-15"]);
    }

    #[test]
    fn rules_this_reader_doesnt_cover_keep_their_first_occurrence() {
        assert!(parse_rule("FREQ=HOURLY").is_none());
        assert!(parse_rule("FREQ=YEARLY;BYWEEKNO=20").is_none());
        assert!(parse_rule("INTERVAL=2").is_none(), "no FREQ");
        let f = feed(&event("UID:h\r\nSUMMARY:X\r\nDTSTART:20261020T100000Z\r\nRRULE:FREQ=HOURLY;COUNT=5\r\n"));
        assert_eq!(starts(&f, "2026-10-20T00:00:00Z", "2026-10-21T00:00:00Z"), vec![ms("2026-10-20T10:00:00Z")]);
    }

    #[test]
    fn excluded_moved_and_cancelled_occurrences() {
        let f = feed(&[
            event("UID:s\r\nSUMMARY:Sync\r\nDTSTART;TZID=Europe/Paris:20261019T140000\r\nDTEND;TZID=Europe/Paris:20261019T143000\r\n\
RRULE:FREQ=DAILY;COUNT=5\r\nEXDATE;TZID=Europe/Paris:20261020T140000\r\n"),
            // Wednesday's is moved to 16:00…
            event("UID:s\r\nRECURRENCE-ID;TZID=Europe/Paris:20261021T140000\r\nSUMMARY:Sync (moved)\r\nDTSTART;TZID=Europe/Paris:20261021T160000\r\nDTEND;TZID=Europe/Paris:20261021T163000\r\n"),
            // …and Thursday's is called off.
            event("UID:s\r\nRECURRENCE-ID;TZID=Europe/Paris:20261022T140000\r\nSTATUS:CANCELLED\r\nSUMMARY:Sync\r\nDTSTART;TZID=Europe/Paris:20261022T140000\r\n"),
        ]
        .concat());
        let all = f.between(ms("2026-10-19T00:00:00Z"), ms("2026-10-25T00:00:00Z"), &paris_winter);
        let got: Vec<(&str, i64)> = all.iter().map(|o| (o.title.as_str(), o.start)).collect();
        assert_eq!(got, vec![
            ("Sync", ms("2026-10-19T12:00:00Z")),
            ("Sync (moved)", ms("2026-10-21T14:00:00Z")),
            ("Sync", ms("2026-10-23T12:00:00Z")),
        ]);
    }

    #[test]
    fn all_day_events_run_from_midnight_to_midnight() {
        let f = feed(&event("UID:d\r\nSUMMARY:Offsite\r\nDTSTART;VALUE=DATE:20261021\r\nDTEND;VALUE=DATE:20261023\r\n"));
        let all = f.between(ms("2026-10-22T10:00:00Z"), ms("2026-10-22T11:00:00Z"), &paris_winter);
        assert_eq!(all.len(), 1, "still running on its second day");
        assert!(all[0].all_day);
        assert_eq!((all[0].start, all[0].end), (ms("2026-10-20T23:00:00Z"), ms("2026-10-22T23:00:00Z")));
        let birthday = feed(&event("UID:b\r\nSUMMARY:Ana\r\nDTSTART;VALUE=DATE:20200301\r\nRRULE:FREQ=YEARLY\r\n"));
        assert_eq!(starts(&birthday, "2026-02-28T00:00:00Z", "2026-03-03T00:00:00Z"), vec![ms("2026-02-28T23:00:00Z")]);
    }

    #[test]
    fn only_what_overlaps_the_window_comes_back() {
        let f = feed(&[
            event("UID:1\r\nSUMMARY:Before\r\nDTSTART:20261019T090000Z\r\nDTEND:20261019T100000Z\r\n"),
            event("UID:2\r\nSUMMARY:Running\r\nDTSTART:20261019T230000Z\r\nDTEND:20261020T010000Z\r\n"),
            event("UID:3\r\nSUMMARY:After\r\nDTSTART:20261021T000000Z\r\n"),
        ]
        .concat());
        let all = f.between(ms("2026-10-20T00:00:00Z"), ms("2026-10-21T00:00:00Z"), &paris_winter);
        assert_eq!(all.iter().map(|o| o.title.as_str()).collect::<Vec<_>>(), ["Running"]);
    }

    // ── People and meetings ──

    #[test]
    fn the_owner_and_the_rooms_are_not_among_the_attendees() {
        let f = Feed::parse(&format!(
            "BEGIN:VCALENDAR\r\nX-WR-CALNAME:me@example.com\r\n{}END:VCALENDAR\r\n",
            event("UID:p\r\nSUMMARY:Plan\r\nDTSTART:20261020T100000Z\r\n\
ATTENDEE;CN=Me;PARTSTAT=DECLINED:mailto:ME@example.com\r\n\
ATTENDEE;CN=Marie Dupont;PARTSTAT=ACCEPTED:mailto:marie@example.com\r\n\
ATTENDEE;CUTYPE=RESOURCE;CN=Room 4:mailto:room4@resource.example.com\r\n\
ATTENDEE;PARTSTAT=NEEDS-ACTION:mailto:paul.martin@example.com\r\n"),
        ))
        .unwrap();
        let o = &f.between(ms("2026-10-20T00:00:00Z"), ms("2026-10-21T00:00:00Z"), &paris_winter)[0];
        assert!(o.declined);
        assert_eq!(o.attendees.iter().map(|a| a.name.as_str()).collect::<Vec<_>>(), ["Marie Dupont", "paul.martin"]);
    }

    #[test]
    fn the_join_link_is_a_video_rooms_and_nothing_else() {
        let google = "Agenda: Q4\n\n-::~:~::~:~:~:~:~::~\nJoin with Google Meet: https://meet.google.com/abc-defg-hij\n\
Join by phone: https://tel.meet/abc-defg-hij?pin=123\nLearn more about Meet at: https://support.google.com/a/users/answer/9282720";
        assert_eq!(join_link(&[google]).as_deref(), Some("https://meet.google.com/abc-defg-hij"));
        let zoom = "Find your local number: https://us02web.zoom.us/u/kd1 Join: https://us02web.zoom.us/j/8123456789?pwd=Xy.";
        assert_eq!(join_link(&[zoom]).as_deref(), Some("https://us02web.zoom.us/j/8123456789?pwd=Xy"));
        let teams = "<https://teams.microsoft.com/l/meetup-join/19%3ameeting_x%40thread.v2/0?context=%7b%7d>";
        assert_eq!(join_link(&[teams]).as_deref(), Some("https://teams.microsoft.com/l/meetup-join/19%3ameeting_x%40thread.v2/0?context=%7b%7d"));
        assert_eq!(join_link(&["https://evil.example.com/meet.google.com/x", "https://meet.google.com.evil.io/abc"]), None);
        assert_eq!(join_link(&["https://zoom.us/", "http://meet.google.com/abc"]), None);
        // Google's own property comes first, before the description's links.
        assert_eq!(join_link(&["https://meet.google.com/aaa-bbbb-ccc", google]).as_deref(), Some("https://meet.google.com/aaa-bbbb-ccc"));
    }

    #[test]
    fn the_agenda_leaves_the_joining_instructions_out() {
        let google = "Agenda: Q4 budget\n- hiring\n\n-::~:~::~:~:~:~:~::~\nJoin with Google Meet: https://meet.google.com/abc-defg-hij\n";
        assert_eq!(agenda(google).as_deref(), Some("Agenda: Q4 budget\n- hiring"));
        let teams = "Bring the numbers.\n________________________________________________________________________________\nMicrosoft Teams meeting\nJoin on your computer";
        assert_eq!(agenda(teams).as_deref(), Some("Bring the numbers."));
        assert_eq!(agenda("<p>Review <b>Q4</b></p><ul><li>hiring &amp; budget</li></ul>").as_deref(), Some("Review Q4\n\nhiring & budget"));
        assert_eq!(agenda("Join Zoom Meeting\nhttps://zoom.us/j/123\n").as_deref(), Some("Join Zoom Meeting"));
        assert_eq!(agenda("-::~:~::~:\nJoin with Google Meet"), None);
    }
}
