//! Goal progress (PRD D3-D6, ARCHITECTURE §7.3, T2.3.2): the weighted actual (R), the weighted
//! expected (E), the pace and each item's share of it. Pure functions; the items' own progress
//! comes from their modules through `ProgressProvider` (ADR-0006).

use tracking::Progress;

/// One linked item's input: its weight (D3) and its progress over the goal's range.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Part {
    pub weight: f64,
    pub progress: Progress,
}

/// One linked item's part of the result, so the calculation can be explained (D6).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PartResult {
    /// `done / total`, from 0 to 1.
    pub actual: f64,
    /// `expected_to_date / total`, from 0 to 1.
    pub expected: f64,
    /// The item's weight over the weights that count, from 0 to 1.
    pub share: f64,
    /// What the item adds to the goal's actual: `share × actual`.
    pub contribution: f64,
    /// `total = 0`: left out of the calculation and shown as "not started" (§7.3).
    pub not_started: bool,
}

/// The goal's two indicators (D4) and the breakdown (D6), in the order of the parts.
#[derive(Debug, Clone, PartialEq)]
pub struct GoalResult {
    /// Weighted actual over the whole range, from 0 to 1.
    pub actual: f64,
    /// Weighted expected share by today, from 0 to 1.
    pub expected: f64,
    /// `actual / expected`; 1 means on plan, and 1 when nothing is expected yet.
    pub pace: f64,
    pub parts: Vec<PartResult>,
}

/// `numerator / total`, clamped to 0..=1; 0 when the total is not positive.
#[must_use]
pub fn ratio(numerator: f64, total: f64) -> f64 {
    if total > 0.0 && numerator.is_finite() {
        (numerator / total).clamp(0.0, 1.0)
    } else {
        0.0
    }
}

fn counts(part: &Part) -> bool {
    part.progress.total > 0.0 && part.weight > 0.0
}

/// The goal's progress from its parts (§7.3): `R = Σ wᵢ·rᵢ / Σ wᵢ`, `E = Σ wᵢ·eᵢ / Σ wᵢ`,
/// `pace = E > 0 ? R / E : 1`. Parts with `total = 0` are left out (§7.3).
#[must_use]
pub fn goal_progress(parts: &[Part]) -> GoalResult {
    let weights: f64 = parts.iter().filter(|p| counts(p)).map(|p| p.weight).sum();
    let mut actual = 0.0;
    let mut expected = 0.0;
    let results = parts
        .iter()
        .map(|part| {
            let r = ratio(part.progress.done, part.progress.total);
            let e = ratio(part.progress.expected_to_date, part.progress.total);
            if !counts(part) || weights <= 0.0 {
                return PartResult {
                    actual: r,
                    expected: e,
                    share: 0.0,
                    contribution: 0.0,
                    not_started: part.progress.total <= 0.0,
                };
            }
            let share = part.weight / weights;
            actual += share * r;
            expected += share * e;
            PartResult {
                actual: r,
                expected: e,
                share,
                contribution: share * r,
                not_started: false,
            }
        })
        .collect();
    let actual = actual.clamp(0.0, 1.0);
    let expected = expected.clamp(0.0, 1.0);
    GoalResult {
        actual,
        expected,
        pace: if expected > 0.0 {
            actual / expected
        } else {
            1.0
        },
        parts: results,
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::float_cmp)]
mod tests {
    use proptest::prelude::*;

    use super::*;

    fn part(weight: f64, done: f64, total: f64, expected: f64) -> Part {
        Part {
            weight,
            progress: Progress {
                done,
                total,
                expected_to_date: expected,
            },
        }
    }

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-9
    }

    #[test]
    fn d3_equal_weights_average_the_items() {
        let result = goal_progress(&[part(1.0, 5.0, 10.0, 5.0), part(1.0, 1.0, 4.0, 2.0)]);
        assert!(close(result.actual, (0.5 + 0.25) / 2.0));
        assert!(close(result.expected, 0.5));
        assert!(close(result.pace, 0.75));
        assert!(close(
            result.parts[0].contribution + result.parts[1].contribution,
            result.actual
        ));
    }

    #[test]
    fn d3_weights_shift_the_average() {
        // A routine at 100 % weighing 3 and a task at 0 % weighing 1.
        let result = goal_progress(&[part(3.0, 8.0, 8.0, 8.0), part(1.0, 0.0, 2.0, 2.0)]);
        assert!(close(result.actual, 0.75));
        assert!(close(result.parts[0].share, 0.75));
        assert!(close(result.parts[1].share, 0.25));
        assert!(close(result.expected, 1.0));
        assert!(close(result.pace, 0.75));
    }

    #[test]
    fn d4_pace_is_one_before_anything_is_expected() {
        let result = goal_progress(&[part(1.0, 0.0, 10.0, 0.0)]);
        assert_eq!(
            (result.actual, result.expected, result.pace),
            (0.0, 0.0, 1.0)
        );
        // Ahead of plan.
        let ahead = goal_progress(&[part(1.0, 6.0, 10.0, 3.0)]);
        assert!(close(ahead.pace, 2.0));
    }

    #[test]
    fn not_started_items_are_left_out() {
        let result = goal_progress(&[part(5.0, 0.0, 0.0, 0.0), part(1.0, 2.0, 4.0, 4.0)]);
        assert!(result.parts[0].not_started);
        assert_eq!(result.parts[0].share, 0.0);
        assert!(close(result.actual, 0.5));
        assert!(close(result.parts[1].share, 1.0));
        let empty = goal_progress(&[]);
        assert_eq!((empty.actual, empty.expected, empty.pace), (0.0, 0.0, 1.0));
        let only_new = goal_progress(&[part(1.0, 0.0, 0.0, 0.0)]);
        assert_eq!(only_new.actual, 0.0);
    }

    #[test]
    fn overshoot_is_capped() {
        let result = goal_progress(&[part(1.0, 15.0, 10.0, 20.0)]);
        assert_eq!((result.actual, result.expected), (1.0, 1.0));
        assert_eq!(ratio(f64::NAN, 1.0), 0.0);
        assert_eq!(ratio(1.0, -1.0), 0.0);
    }

    fn parts() -> impl Strategy<Value = Vec<Part>> {
        prop::collection::vec(
            (0.01_f64..100.0, 0.0_f64..50.0, 0.0_f64..40.0, 0.0_f64..50.0)
                .prop_map(|(w, d, t, e)| part(w, d, t, e)),
            0..12,
        )
    }

    proptest! {
        #[test]
        fn d3_actual_is_the_weighted_mean_and_stays_in_range(input in parts()) {
            let result = goal_progress(&input);
            prop_assert!((0.0..=1.0).contains(&result.actual));
            prop_assert!((0.0..=1.0).contains(&result.expected));
            prop_assert!(result.pace >= 0.0);
            let counted: Vec<&Part> = input.iter().filter(|p| p.progress.total > 0.0).collect();
            let weights: f64 = counted.iter().map(|p| p.weight).sum();
            let mean = if weights > 0.0 {
                counted.iter().map(|p| p.weight * ratio(p.progress.done, p.progress.total)).sum::<f64>() / weights
            } else {
                0.0
            };
            prop_assert!(close(result.actual, mean));
            // D6: the contributions add up to the actual; shares add up to 1 when anything counts.
            let sum: f64 = result.parts.iter().map(|p| p.contribution).sum();
            prop_assert!(close(sum, result.actual));
            let shares: f64 = result.parts.iter().map(|p| p.share).sum();
            let expected_shares = if weights > 0.0 { 1.0 } else { 0.0 };
            prop_assert!(close(shares, expected_shares));
            prop_assert_eq!(result.parts.len(), input.len());
        }

        #[test]
        fn d3_scaling_every_weight_changes_nothing(input in parts(), factor in 0.1_f64..50.0) {
            let scaled: Vec<Part> = input.iter().map(|p| Part { weight: p.weight * factor, ..*p }).collect();
            let a = goal_progress(&input);
            let b = goal_progress(&scaled);
            prop_assert!(close(a.actual, b.actual));
            prop_assert!(close(a.expected, b.expected));
        }

        #[test]
        fn d4_pace_is_actual_over_expected(input in parts()) {
            let result = goal_progress(&input);
            if result.expected > 0.0 {
                prop_assert!(close(result.pace * result.expected, result.actual));
            } else {
                prop_assert_eq!(result.pace, 1.0);
            }
        }
    }
}
