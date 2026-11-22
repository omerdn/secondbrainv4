//! The supported RRULE subset (RFC 5545 §3.3.10) and date matching (PRD R1, ADR-0008).
//!
//! Supported parts: `FREQ` (DAILY, WEEKLY, MONTHLY, YEARLY), `INTERVAL`, `BYDAY` (WEEKLY only,
//! plain weekdays), `BYMONTHDAY` (MONTHLY and YEARLY; 1..=31 or -31..=-1, `-1` = last day of the
//! month) and `BYMONTH` (YEARLY only). Everything else (`COUNT`, `UNTIL`, `WKST`, ordinals such as
//! `1MO`, …) is rejected: start and end dates come from the plan versions instead.
//!
//! As in the RFC, a day that does not exist in a month ("the 31st", "29 February") is skipped.
//! The first occurrence is anchored at the plan version's first day (DTSTART), which also fixes
//! the phase of `INTERVAL` and the defaults of missing `BY*` parts.

use std::fmt;
use std::str::FromStr;

use chrono::{Datelike, NaiveDate, Weekday};

/// Why a rule string was rejected.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum RuleError {
    #[error("rule is empty")]
    Empty,
    #[error("`{0}` is not a NAME=VALUE part")]
    Malformed(String),
    #[error("FREQ is missing")]
    MissingFreq,
    #[error("{0} appears more than once")]
    Duplicate(String),
    #[error("{0} is not supported")]
    Unsupported(String),
    #[error("invalid value `{value}` for {part}")]
    InvalidValue { part: &'static str, value: String },
    #[error("{part} cannot be used with FREQ={freq}")]
    NotAllowed { part: &'static str, freq: Frequency },
}

/// `FREQ`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Frequency {
    Daily,
    Weekly,
    Monthly,
    Yearly,
}

impl Frequency {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Daily => "DAILY",
            Self::Weekly => "WEEKLY",
            Self::Monthly => "MONTHLY",
            Self::Yearly => "YEARLY",
        }
    }
}

impl fmt::Display for Frequency {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// One recurrence rule. Build it with the constructors or parse it from RRULE text; both keep the
/// `BY*` lists sorted and free of duplicates, so equal rules compare equal and print the same.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Rule {
    freq: Frequency,
    interval: u32,
    by_day: Vec<Weekday>,
    by_month_day: Vec<i8>,
    by_month: Vec<u8>,
}

const fn weekday_code(day: Weekday) -> &'static str {
    match day {
        Weekday::Mon => "MO",
        Weekday::Tue => "TU",
        Weekday::Wed => "WE",
        Weekday::Thu => "TH",
        Weekday::Fri => "FR",
        Weekday::Sat => "SA",
        Weekday::Sun => "SU",
    }
}

fn parse_weekday(code: &str) -> Option<Weekday> {
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

/// Number of days in the month of `date`.
fn days_in_month(year: i32, month: u32) -> u32 {
    let (next_year, next_month) = if month == 12 {
        (year + 1, 1)
    } else {
        (year, month + 1)
    };
    NaiveDate::from_ymd_opt(next_year, next_month, 1)
        .and_then(|d| d.pred_opt())
        .map_or(31, |d| d.day())
}

/// Whether `date` is the `month_day`-th day of its month (negative: counted from the end).
fn is_month_day(date: NaiveDate, month_day: i8) -> bool {
    let day = i64::from(date.day());
    let wanted = i64::from(month_day);
    if wanted > 0 {
        day == wanted
    } else {
        let len = i64::from(days_in_month(date.year(), date.month()));
        day == len + 1 + wanted
    }
}

fn months_since(start: NaiveDate, date: NaiveDate) -> i64 {
    (i64::from(date.year()) * 12 + i64::from(date.month0()))
        - (i64::from(start.year()) * 12 + i64::from(start.month0()))
}

fn week_start(date: NaiveDate) -> NaiveDate {
    date - chrono::Days::new(u64::from(date.weekday().num_days_from_monday()))
}

fn normalized<T: Ord>(mut values: Vec<T>) -> Vec<T> {
    values.sort_unstable();
    values.dedup();
    values
}

fn normalized_weekdays(days: impl IntoIterator<Item = Weekday>) -> Vec<Weekday> {
    let mut days: Vec<Weekday> = days.into_iter().collect();
    days.sort_unstable_by_key(Weekday::num_days_from_monday);
    days.dedup();
    days
}

impl Rule {
    /// Every `interval` days.
    #[must_use]
    pub fn daily(interval: u32) -> Self {
        Self::bare(Frequency::Daily, interval)
    }

    /// Every `interval` weeks on `days` (the start date's weekday if empty).
    #[must_use]
    pub fn weekly(interval: u32, days: impl IntoIterator<Item = Weekday>) -> Self {
        Self {
            by_day: normalized_weekdays(days),
            ..Self::bare(Frequency::Weekly, interval)
        }
    }

    /// Every `interval` months on `month_days` (the start date's day if empty; `-1` = last day).
    ///
    /// # Errors
    /// A month day of 0 or outside -31..=31.
    pub fn monthly(
        interval: u32,
        month_days: impl IntoIterator<Item = i8>,
    ) -> Result<Self, RuleError> {
        let rule = Self {
            by_month_day: normalized(month_days.into_iter().collect()),
            ..Self::bare(Frequency::Monthly, interval)
        };
        rule.validate()?;
        Ok(rule)
    }

    /// Every `interval` years on `month_day` of `month` (e.g. 8 March).
    ///
    /// # Errors
    /// A month outside 1..=12 or an invalid month day.
    pub fn yearly(interval: u32, month: u8, month_day: i8) -> Result<Self, RuleError> {
        let rule = Self {
            by_month: vec![month],
            by_month_day: vec![month_day],
            ..Self::bare(Frequency::Yearly, interval)
        };
        rule.validate()?;
        Ok(rule)
    }

    fn bare(freq: Frequency, interval: u32) -> Self {
        Self {
            freq,
            interval: interval.max(1),
            by_day: Vec::new(),
            by_month_day: Vec::new(),
            by_month: Vec::new(),
        }
    }

    #[must_use]
    pub const fn frequency(&self) -> Frequency {
        self.freq
    }

    #[must_use]
    pub const fn interval(&self) -> u32 {
        self.interval
    }

    fn validate(&self) -> Result<(), RuleError> {
        if let Some(bad) = self
            .by_month_day
            .iter()
            .find(|d| **d == 0 || !(-31..=31).contains(*d))
        {
            return Err(RuleError::InvalidValue {
                part: "BYMONTHDAY",
                value: bad.to_string(),
            });
        }
        if let Some(bad) = self.by_month.iter().find(|m| !(1..=12).contains(*m)) {
            return Err(RuleError::InvalidValue {
                part: "BYMONTH",
                value: bad.to_string(),
            });
        }
        let not_allowed = |part| {
            Err(RuleError::NotAllowed {
                part,
                freq: self.freq,
            })
        };
        if !self.by_day.is_empty() && self.freq != Frequency::Weekly {
            return not_allowed("BYDAY");
        }
        if !self.by_month_day.is_empty()
            && matches!(self.freq, Frequency::Daily | Frequency::Weekly)
        {
            return not_allowed("BYMONTHDAY");
        }
        if !self.by_month.is_empty() && self.freq != Frequency::Yearly {
            return not_allowed("BYMONTH");
        }
        Ok(())
    }

    /// Whether the rule, started on `start`, has an occurrence on `date`.
    #[must_use]
    pub fn matches(&self, start: NaiveDate, date: NaiveDate) -> bool {
        if date < start {
            return false;
        }
        let interval = i64::from(self.interval);
        match self.freq {
            Frequency::Daily => (date - start).num_days() % interval == 0,
            Frequency::Weekly => {
                let weeks = (week_start(date) - week_start(start)).num_days() / 7;
                let on_day = if self.by_day.is_empty() {
                    date.weekday() == start.weekday()
                } else {
                    self.by_day.contains(&date.weekday())
                };
                weeks % interval == 0 && on_day
            }
            Frequency::Monthly => {
                months_since(start, date) % interval == 0 && self.on_month_day(start, date)
            }
            Frequency::Yearly => {
                let in_month = if self.by_month.is_empty() {
                    date.month() == start.month()
                } else {
                    self.by_month.iter().any(|m| u32::from(*m) == date.month())
                };
                i64::from(date.year() - start.year()) % interval == 0
                    && in_month
                    && self.on_month_day(start, date)
            }
        }
    }

    fn on_month_day(&self, start: NaiveDate, date: NaiveDate) -> bool {
        if self.by_month_day.is_empty() {
            date.day() == start.day()
        } else {
            self.by_month_day.iter().any(|d| is_month_day(date, *d))
        }
    }
}

impl fmt::Display for Rule {
    /// Canonical RRULE value, e.g. `FREQ=WEEKLY;INTERVAL=2;BYDAY=MO,TH`.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "FREQ={}", self.freq)?;
        if self.interval != 1 {
            write!(f, ";INTERVAL={}", self.interval)?;
        }
        if !self.by_month.is_empty() {
            let list: Vec<String> = self.by_month.iter().map(u8::to_string).collect();
            write!(f, ";BYMONTH={}", list.join(","))?;
        }
        if !self.by_month_day.is_empty() {
            let list: Vec<String> = self.by_month_day.iter().map(i8::to_string).collect();
            write!(f, ";BYMONTHDAY={}", list.join(","))?;
        }
        if !self.by_day.is_empty() {
            let list: Vec<&str> = self.by_day.iter().map(|d| weekday_code(*d)).collect();
            write!(f, ";BYDAY={}", list.join(","))?;
        }
        Ok(())
    }
}

fn parse_list<T>(
    part: &'static str,
    value: &str,
    parse: impl Fn(&str) -> Option<T>,
) -> Result<Vec<T>, RuleError> {
    value
        .split(',')
        .map(|v| {
            parse(v).ok_or_else(|| RuleError::InvalidValue {
                part,
                value: v.to_owned(),
            })
        })
        .collect()
}

impl FromStr for Rule {
    type Err = RuleError;

    /// Parses an RRULE value, with or without the `RRULE:` prefix. Part names are case-insensitive.
    fn from_str(s: &str) -> Result<Self, RuleError> {
        let s = s.trim();
        let s = s.strip_prefix("RRULE:").unwrap_or(s);
        if s.is_empty() {
            return Err(RuleError::Empty);
        }
        let mut freq = None;
        let mut interval = None;
        let mut by_day = None;
        let mut by_month_day = None;
        let mut by_month = None;
        for part in s.split(';') {
            let (name, value) = part
                .split_once('=')
                .ok_or_else(|| RuleError::Malformed(part.to_owned()))?;
            let name = name.trim().to_ascii_uppercase();
            let value = value.trim().to_ascii_uppercase();
            let duplicate = || RuleError::Duplicate(name.clone());
            match name.as_str() {
                "FREQ" => {
                    let parsed = match value.as_str() {
                        "DAILY" => Frequency::Daily,
                        "WEEKLY" => Frequency::Weekly,
                        "MONTHLY" => Frequency::Monthly,
                        "YEARLY" => Frequency::Yearly,
                        _ => {
                            return Err(RuleError::InvalidValue {
                                part: "FREQ",
                                value,
                            });
                        }
                    };
                    if freq.replace(parsed).is_some() {
                        return Err(duplicate());
                    }
                }
                "INTERVAL" => {
                    let parsed =
                        value
                            .parse::<u32>()
                            .ok()
                            .filter(|n| *n >= 1)
                            .ok_or_else(|| RuleError::InvalidValue {
                                part: "INTERVAL",
                                value: value.clone(),
                            })?;
                    if interval.replace(parsed).is_some() {
                        return Err(duplicate());
                    }
                }
                "BYDAY" => {
                    let parsed = parse_list("BYDAY", &value, parse_weekday)?;
                    if by_day.replace(parsed).is_some() {
                        return Err(duplicate());
                    }
                }
                "BYMONTHDAY" => {
                    let parsed = parse_list("BYMONTHDAY", &value, |v| v.parse::<i8>().ok())?;
                    if by_month_day.replace(parsed).is_some() {
                        return Err(duplicate());
                    }
                }
                "BYMONTH" => {
                    let parsed = parse_list("BYMONTH", &value, |v| v.parse::<u8>().ok())?;
                    if by_month.replace(parsed).is_some() {
                        return Err(duplicate());
                    }
                }
                _ => return Err(RuleError::Unsupported(name)),
            }
        }
        let rule = Self {
            freq: freq.ok_or(RuleError::MissingFreq)?,
            interval: interval.unwrap_or(1),
            by_day: normalized_weekdays(by_day.unwrap_or_default()),
            by_month_day: normalized(by_month_day.unwrap_or_default()),
            by_month: normalized(by_month.unwrap_or_default()),
        };
        rule.validate()?;
        Ok(rule)
    }
}

/// The rules of one plan version: the union of one or more RRULEs, one per line. Several lines
/// are needed for "yearly on selected dates" (R1), which a single RRULE cannot express.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct RuleSet(Vec<Rule>);

impl RuleSet {
    /// # Errors
    /// [`RuleError::Empty`] if `rules` is empty.
    pub fn new(rules: impl IntoIterator<Item = Rule>) -> Result<Self, RuleError> {
        let mut rules: Vec<Rule> = rules.into_iter().collect();
        if rules.is_empty() {
            return Err(RuleError::Empty);
        }
        rules.dedup();
        Ok(Self(rules))
    }

    #[must_use]
    pub fn rules(&self) -> &[Rule] {
        &self.0
    }

    /// Whether any rule, started on `start`, has an occurrence on `date`.
    #[must_use]
    pub fn matches(&self, start: NaiveDate, date: NaiveDate) -> bool {
        self.0.iter().any(|r| r.matches(start, date))
    }
}

impl From<Rule> for RuleSet {
    fn from(rule: Rule) -> Self {
        Self(vec![rule])
    }
}

impl fmt::Display for RuleSet {
    /// One canonical RRULE value per line; this is the text stored in `routine_schedules.rrule`.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (i, rule) in self.0.iter().enumerate() {
            if i > 0 {
                f.write_str("\n")?;
            }
            write!(f, "{rule}")?;
        }
        Ok(())
    }
}

impl FromStr for RuleSet {
    type Err = RuleError;

    /// Parses one RRULE per non-blank line.
    fn from_str(s: &str) -> Result<Self, RuleError> {
        let rules = s
            .lines()
            .filter(|l| !l.trim().is_empty())
            .map(str::parse)
            .collect::<Result<Vec<Rule>, _>>()?;
        Self::new(rules)
    }
}

#[cfg(test)]
pub(crate) mod tests;
