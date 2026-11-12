//! Scoring formats (ADR 0005).

use serde::{Deserialize, Serialize};

use super::Side;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SetCount {
    /// Every set is played, e.g. MTFV: 2:0, 1:1 and 0:2 are all results.
    Fixed(u16),
    /// The game ends once one side has won a majority.
    BestOf(u16),
}

impl SetCount {
    pub fn count(self) -> u16 {
        match self {
            SetCount::Fixed(n) | SetCount::BestOf(n) => n,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Format {
    Sets {
        sets: SetCount,
        points: u16,
        win_by: u16,
        cap: Option<u16>,
    },
    Race {
        target: u16,
        segment: u16,
        draw_at: Option<u16>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Outcome {
    Home,
    Away,
    Draw,
}

impl Outcome {
    pub fn from_points(home: u16, away: u16) -> Self {
        match home.cmp(&away) {
            std::cmp::Ordering::Greater => Outcome::Home,
            std::cmp::Ordering::Less => Outcome::Away,
            std::cmp::Ordering::Equal => Outcome::Draw,
        }
    }
}

impl Format {
    /// MTFV Landesliga: Race to 42 in segments of 6, draw at 41:41.
    pub fn mtfv_race() -> Self {
        Format::Race {
            target: 42,
            segment: 6,
            draw_at: Some(41),
        }
    }

    /// MTFV Verbandsliga and Bezirksliga: two sets to 5, two-goal lead.
    pub fn mtfv_sets() -> Self {
        Format::Sets {
            sets: SetCount::Fixed(2),
            points: 5,
            win_by: 2,
            cap: None,
        }
    }

    /// Winner of a single set, if it is over. Always `None` for a race.
    pub fn set_winner(&self, home: u16, away: u16) -> Option<Side> {
        let Format::Sets {
            points,
            win_by,
            cap,
            ..
        } = *self
        else {
            return None;
        };
        let (leader, lead_score, trail_score) = if home >= away {
            (Side::Home, home, away)
        } else {
            (Side::Away, away, home)
        };
        if cap.is_some_and(|c| lead_score >= c) && lead_score > trail_score {
            return Some(leader);
        }
        (lead_score >= points && lead_score - trail_score >= win_by.max(1)).then_some(leader)
    }

    /// Result of a race at the given running totals, if it is over.
    pub fn race_outcome(&self, home: u16, away: u16) -> Option<Outcome> {
        let Format::Race {
            target, draw_at, ..
        } = *self
        else {
            return None;
        };
        if home >= target || away >= target {
            return Some(Outcome::from_points(home, away));
        }
        if draw_at.is_some_and(|d| home >= d && away >= d) {
            return Some(Outcome::Draw);
        }
        None
    }

    /// Segment index (0-based) for the given running totals: it advances when
    /// the larger total reaches the next multiple of the segment length.
    pub fn race_segment(&self, home: u16, away: u16, segments: usize) -> usize {
        let Format::Race { segment, .. } = *self else {
            return 0;
        };
        if segment == 0 || segments == 0 {
            return 0;
        }
        let reached = usize::from(home.max(away) / segment);
        reached.min(segments - 1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sets_need_lead_of_two() {
        let f = Format::mtfv_sets();
        assert_eq!(f.set_winner(5, 3), Some(Side::Home));
        assert_eq!(f.set_winner(5, 4), None);
        assert_eq!(f.set_winner(6, 4), Some(Side::Home));
        assert_eq!(f.set_winner(2, 5), Some(Side::Away));
        assert_eq!(f.set_winner(4, 4), None);
    }

    #[test]
    fn sets_cap_ends_without_lead() {
        let f = Format::Sets {
            sets: SetCount::BestOf(3),
            points: 5,
            win_by: 2,
            cap: Some(8),
        };
        assert_eq!(f.set_winner(7, 7), None);
        assert_eq!(f.set_winner(8, 7), Some(Side::Home));
    }

    #[test]
    fn race_segment_follows_leader() {
        let f = Format::mtfv_race();
        // ends of segments from the example in ADR 0005
        assert_eq!(f.race_segment(6, 3, 7), 1);
        assert_eq!(f.race_segment(5, 3, 7), 0);
        assert_eq!(f.race_segment(12, 10, 7), 2);
        assert_eq!(f.race_segment(35, 36, 7), 6);
        assert_eq!(f.race_segment(41, 40, 7), 6);
        assert_eq!(f.race_segment(42, 40, 7), 6);
    }

    #[test]
    fn race_ends_at_target_or_draw() {
        let f = Format::mtfv_race();
        assert_eq!(f.race_outcome(42, 40), Some(Outcome::Home));
        assert_eq!(f.race_outcome(30, 42), Some(Outcome::Away));
        assert_eq!(f.race_outcome(41, 41), Some(Outcome::Draw));
        assert_eq!(f.race_outcome(41, 40), None);
    }

    #[test]
    fn format_serializes_tagged() {
        let json = serde_json::to_value(Format::mtfv_sets()).unwrap();
        assert_eq!(json["kind"], "sets");
        assert_eq!(json["sets"]["fixed"], 2);
        let back: Format = serde_json::from_value(json).unwrap();
        assert_eq!(back, Format::mtfv_sets());
    }
}
