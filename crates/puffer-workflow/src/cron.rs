use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

/// Parsed five-field cron expression.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CronExpression {
    minute: Field,
    hour: Field,
    day_of_month: Field,
    month: Field,
    day_of_week: Field,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Field {
    values: BTreeSet<u32>,
}

impl CronExpression {
    /// Parses a local-time five-field cron expression.
    pub fn parse(expr: &str) -> Result<Self> {
        let parts: Vec<_> = expr.split_whitespace().collect();
        if parts.len() != 5 {
            return Err(anyhow!("cron must have exactly five fields"));
        }
        Ok(Self {
            minute: parse_field(parts[0], 0, 59)?,
            hour: parse_field(parts[1], 0, 23)?,
            day_of_month: parse_field(parts[2], 1, 31)?,
            month: parse_field(parts[3], 1, 12)?,
            day_of_week: parse_field(parts[4], 0, 7)?,
        })
    }

    /// Returns true when the provided local date/time matches this cron expression.
    pub fn matches(&self, minute: u32, hour: u32, day: u32, month: u32, weekday: u32) -> bool {
        let weekday_match = self.day_of_week.values.contains(&weekday)
            || (weekday == 0 && self.day_of_week.values.contains(&7));
        self.minute.values.contains(&minute)
            && self.hour.values.contains(&hour)
            && self.day_of_month.values.contains(&day)
            && self.month.values.contains(&month)
            && weekday_match
    }
}

/// Returns true when a five-field cron expression matches the provided local date/time.
pub fn cron_matches(
    expr: &str,
    minute: u32,
    hour: u32,
    day: u32,
    month: u32,
    weekday: u32,
) -> Result<bool> {
    Ok(CronExpression::parse(expr)?.matches(minute, hour, day, month, weekday))
}

fn parse_field(raw: &str, min: u32, max: u32) -> Result<Field> {
    let mut values = BTreeSet::new();
    for part in raw.split(',') {
        let (base, step) = if let Some((base, step)) = part.split_once('/') {
            let step = step
                .parse::<u32>()
                .map_err(|_| anyhow!("invalid cron step `{step}`"))?;
            if step == 0 {
                return Err(anyhow!("cron step must be greater than zero"));
            }
            (base, step)
        } else {
            (part, 1)
        };
        let (start, end) = if base == "*" {
            (min, max)
        } else if let Some((start, end)) = base.split_once('-') {
            (parse_num(start, min, max)?, parse_num(end, min, max)?)
        } else {
            let value = parse_num(base, min, max)?;
            (value, value)
        };
        if start > end {
            return Err(anyhow!("invalid cron range `{base}`"));
        }
        for value in (start..=end).step_by(step as usize) {
            values.insert(value);
        }
    }
    Ok(Field { values })
}

fn parse_num(raw: &str, min: u32, max: u32) -> Result<u32> {
    let value = raw
        .parse::<u32>()
        .map_err(|_| anyhow!("invalid cron value `{raw}`"))?;
    if value < min || value > max {
        return Err(anyhow!("cron value `{raw}` out of range {min}-{max}"));
    }
    Ok(value)
}

/// In-memory cron duplicate prevention keyed by workflow slug and minute bucket.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CronDeduper {
    fired: BTreeSet<String>,
}

impl CronDeduper {
    /// Returns true if this workflow/minute combination has not fired before.
    pub fn mark_if_new(&mut self, workflow_slug: &str, minute_epoch: i64) -> bool {
        self.fired.insert(format!("{workflow_slug}:{minute_epoch}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_steps_and_lists() {
        assert!(cron_matches("*/15 9,10 * * 1-5", 30, 9, 10, 4, 2).unwrap());
        assert!(!cron_matches("*/15 9,10 * * 1-5", 31, 9, 10, 4, 2).unwrap());
    }

    #[test]
    fn dedupes_per_workflow_minute() {
        let mut deduper = CronDeduper::default();
        assert!(deduper.mark_if_new("a", 1));
        assert!(!deduper.mark_if_new("a", 1));
        assert!(deduper.mark_if_new("a", 2));
    }
}
