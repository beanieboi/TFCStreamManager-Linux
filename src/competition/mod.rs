//! Source-independent domain: match day structure, scoring formats and the
//! event log. See ADRs 0004–0006 in architecture-decisions/tfc-stream-manager.

// Wired into the web server and UI in the next step.
#![allow(dead_code, unused_imports)]

mod format;
mod match_day;
mod modus;

pub use format::{Format, Outcome, SetCount};
pub use match_day::{
    CommandError, Event, Lineups, MatchDay, Player, Progress, Scoreboard, SideView, Source, Team,
    UnitResult, UnitView,
};
pub use modus::{ModusError, Side, Slot, SlotKind, UnitSpec, game_groups, parse_modus};

#[cfg(test)]
pub(crate) use match_day::tests as test_support;
