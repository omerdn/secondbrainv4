//! The weekly summary as text (OZ1, T4.3.5), for the e-mail; the app renders its own view.

use std::fmt::Write as _;

use crate::model::WeeklySummary;

fn duration(minutes: u32) -> String {
    match (minutes / 60, minutes % 60) {
        (0, m) => format!("{m} dk"),
        (h, 0) => format!("{h} sa"),
        (h, m) => format!("{h} sa {m} dk"),
    }
}

/// The summary in Turkish plain text, one section per kind of data, empty ones left out.
#[must_use]
pub fn summary_text(summary: &WeeklySummary) -> String {
    let mut out = String::new();
    if !summary.routines.is_empty() {
        let _ = writeln!(out, "Rutinler");
        for routine in &summary.routines {
            let _ = write!(
                out,
                "- {}: {}/{} gün",
                routine.title, routine.done_days, routine.scheduled_days
            );
            if routine.streak > 0 {
                let _ = write!(out, " · seri {}", routine.streak);
            }
            out.push('\n');
        }
        out.push('\n');
    }
    if !summary.goals.is_empty() {
        let _ = writeln!(out, "Hedefler");
        for goal in &summary.goals {
            let _ = writeln!(
                out,
                "- {}: %{} (beklenen %{})",
                goal.title, goal.actual_percent, goal.expected_percent
            );
        }
        out.push('\n');
    }
    if !summary.sessions.is_empty() {
        let _ = writeln!(
            out,
            "Çalışma oturumları: toplam {}",
            duration(summary.work_minutes)
        );
        for session in &summary.sessions {
            let _ = writeln!(out, "- {}: {}", session.title, duration(session.minutes));
        }
        out.push('\n');
    }
    if !summary.forgotten.is_empty() {
        let _ = writeln!(out, "Bir süredir dokunulmayanlar");
        for title in &summary.forgotten {
            let _ = writeln!(out, "- {title}");
        }
        out.push('\n');
    }
    out
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use chrono::NaiveDate;
    use uuid::Uuid;

    use super::*;
    use crate::model::{SummaryGoal, SummaryRoutine, SummarySession};

    #[test]
    fn oz1_text_lists_each_part_in_turkish_and_skips_empty_ones() {
        let summary = WeeklySummary {
            week_start: NaiveDate::from_ymd_opt(2026, 11, 16).unwrap(),
            week_end: NaiveDate::from_ymd_opt(2026, 11, 22).unwrap(),
            routines: vec![SummaryRoutine {
                routine_id: Uuid::now_v7(),
                title: "Su iç".into(),
                done_days: 5,
                scheduled_days: 7,
                streak: 3,
            }],
            goals: vec![SummaryGoal {
                goal_id: Uuid::now_v7(),
                title: "Kitap".into(),
                actual_percent: 40,
                expected_percent: 50,
            }],
            sessions: vec![SummarySession {
                item_id: None,
                title: "Rapor".into(),
                minutes: 95,
            }],
            work_minutes: 95,
            forgotten: Vec::new(),
        };
        let text = summary_text(&summary);
        assert!(text.contains("- Su iç: 5/7 gün · seri 3"));
        assert!(text.contains("- Kitap: %40 (beklenen %50)"));
        assert!(text.contains("toplam 1 sa 35 dk"));
        assert!(!text.contains("dokunulmayanlar"));
        assert_eq!(duration(60), "1 sa");
        assert_eq!(duration(5), "5 dk");
    }
}
