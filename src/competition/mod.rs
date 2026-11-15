//! Source-independent domain: the match on the table, scoring formats and the
//! event log. See ADRs 0004–0006 in architecture-decisions/tfc-stream-manager.

mod format;
mod modus;
mod table_match;

pub use format::{Format, MatchFormat, Outcome, SetCount};
pub use modus::{
    ModusError, Side, Slot, SlotKind, UnitSpec, game_groups, kinds_from_letters, kinds_to_letters,
    parse_modus, race_units,
};
#[cfg(test)]
pub use table_match::UnitView;
pub use table_match::{
    CommandError, Competition, Event, Labels, Player, Scoreboard, SideView, TableMatch, Team,
};

#[cfg(test)]
pub(crate) use table_match::tests as test_support;
