//! Scoring formats (ADR 0005).

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SetCount {
    /// Every set is played, e.g. MTFV: 2:0, 1:1 and 0:2 are all results.
    Fixed(u16),
    /// The game ends once one side has won a majority.
    BestOf(u16),
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
    /// Goals of the current set; sets are entered by hand. Set rules (e.g.
    /// overtime in the last set) vary too much to derive them.
    ManualSets,
}

/// The format of a single game, as the operator sets it for Kickertool and
/// Frei matches (ADR 0012). MTFV formats come from the league instead.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum MatchFormat {
    /// Goals per set, sets entered by hand. Fields of older versions
    /// (`best_of`, `points`) are ignored.
    Sets {},
    /// First to `target` goals, in segments of `segment` goals (0: none).
    Race {
        target: u16,
        #[serde(default)]
        segment: u16,
    },
}

impl Default for MatchFormat {
    fn default() -> Self {
        MatchFormat::Sets {}
    }
}

impl MatchFormat {
    pub fn format(self) -> Format {
        match self {
            MatchFormat::Sets {} => Format::ManualSets,
            MatchFormat::Race { target, segment } => Format::Race {
                target: target.max(1),
                segment,
                draw_at: None,
            },
        }
    }

    /// The single-game format a [`Format`] stands for.
    pub fn from_format(format: Format) -> Self {
        match format {
            Format::Race {
                target, segment, ..
            } => MatchFormat::Race { target, segment },
            Format::Sets { .. } | Format::ManualSets => MatchFormat::Sets {},
        }
    }
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
    #[cfg(test)]
    pub fn mtfv_race() -> Self {
        Format::Race {
            target: 42,
            segment: 6,
            draw_at: Some(41),
        }
    }

    /// MTFV Verbandsliga and Bezirksliga: two sets to 5, two-goal lead.
    #[cfg(test)]
    pub fn mtfv_sets() -> Self {
        Format::Sets {
            sets: SetCount::Fixed(2),
            points: 5,
            win_by: 2,
            cap: Some(8),
        }
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

    /// The race segment shown on the overlay, 1-based, and how many there
    /// are: the target split into segments of `segment` goals, the last one
    /// possibly shorter. `None` without segments.
    pub fn race_segment_of(&self, home: u16, away: u16) -> Option<(u16, u16)> {
        let Format::Race {
            target, segment, ..
        } = *self
        else {
            return None;
        };
        if segment == 0 {
            return None;
        }
        let count = target.div_ceil(segment).max(1);
        let current = (home.max(away) / segment).min(count - 1) + 1;
        Some((current, count))
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
    fn match_format_round_trips() {
        for f in [
            MatchFormat::Sets {},
            MatchFormat::Race {
                target: 10,
                segment: 5,
            },
        ] {
            assert_eq!(MatchFormat::from_format(f.format()), f);
        }
    }

    #[test]
    fn segments_from_target_and_length() {
        let race = Format::mtfv_race();
        assert_eq!(race.race_segment_of(0, 0), Some((1, 7)));
        assert_eq!(race.race_segment_of(5, 3), Some((1, 7)));
        assert_eq!(race.race_segment_of(6, 3), Some((2, 7)));
        assert_eq!(race.race_segment_of(41, 40), Some((7, 7)));
        assert_eq!(
            race.race_segment_of(42, 40),
            Some((7, 7)),
            "the end stays in the last"
        );
        let five = MatchFormat::Race {
            target: 42,
            segment: 5,
        }
        .format();
        assert_eq!(five.race_segment_of(0, 0), Some((1, 9)), "42 in fives: 9");
        let none = MatchFormat::Race {
            target: 10,
            segment: 0,
        }
        .format();
        assert_eq!(none.race_segment_of(3, 3), None);
        assert_eq!(MatchFormat::Sets {}.format().race_segment_of(1, 1), None);
    }

    #[test]
    fn old_sets_format_still_reads() {
        let f: MatchFormat =
            serde_json::from_str(r#"{"kind":"sets","best_of":3,"points":5}"#).unwrap();
        assert_eq!(f, MatchFormat::Sets {});
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
