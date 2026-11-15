//! The match on the table (ADR 0007, 0012): two sides, the games, lineups
//! and an append-only event log (ADR 0004, 0006). A Kickertool or Frei match
//! is one game; an MTFV Begegnung has the games of its `modus`.
//!
//! Scores are never stored directly. They are derived by replaying the event
//! log through the format rules, so undo works across set and segment
//! boundaries and a restart only needs the log.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

use super::modus::{SlotKind, game_groups};
use super::{Format, MatchFormat, Outcome, Side, Slot, UnitSpec};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Player {
    pub id: String,
    pub first_name: String,
    pub last_name: String,
}

impl Player {
    pub fn display_name(&self) -> String {
        format!("{} {}", self.first_name, self.last_name)
            .trim()
            .to_owned()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Team {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub roster: Vec<Player>,
}

impl Team {
    /// A team known only by name, e.g. a Kickertool entry or typed in.
    pub fn named(name: impl Into<String>) -> Self {
        Self {
            id: String::new(),
            name: name.into(),
            roster: Vec::new(),
        }
    }
}

/// What the overlay shows around the score. Filled by the competition or
/// entered (ADR 0012).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Labels {
    pub tournament: String,
    pub round: String,
    pub group: String,
    pub discipline: String,
    pub table: String,
}

impl Labels {
    pub fn title(&self) -> String {
        [self.tournament.as_str(), self.round.as_str()]
            .into_iter()
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>()
            .join(" · ")
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Event {
    Goal {
        side: Side,
    },
    /// Cancels the latest event that is not already cancelled.
    Undo,
    /// Operator jumps to a unit (set-based formats only).
    Select {
        unit: usize,
    },
    /// Operator ends the match by hand.
    Finish,
    /// Sets the score of the current set; in a race, the running totals.
    Correct {
        home: u16,
        away: u16,
    },
    /// A set entered by hand: one more for `side`, the next set starts at
    /// 0:0 (manual sets only).
    SetWon {
        side: Side,
    },
    /// Sets the sets won (manual sets only).
    CorrectSets {
        home: u16,
        away: u16,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Competition {
    /// "Frei" in the UI: everything is entered.
    Manual,
    Kickertool {
        tournament_id: String,
        table_id: String,
        match_id: String,
    },
    Mtfv {
        veranstaltung_id: u32,
        begegnung_id: u32,
        /// `teamspiel_id` per unit, in unit order.
        teamspiel_ids: Vec<u64>,
    },
}

impl Competition {
    /// The name overlay templates list in `tfc:sources` (ADR 0010).
    pub fn key(&self) -> &'static str {
        match self {
            Competition::Manual => "manual",
            Competition::Kickertool { .. } => "kickertool",
            Competition::Mtfv { .. } => "mtfv",
        }
    }

    /// Kickertool and Frei matches are one game in a format the operator
    /// sets; an MTFV format comes from the league.
    pub fn is_single_game(&self) -> bool {
        !matches!(self, Competition::Mtfv { .. })
    }
}

/// Per slot, roster ids or typed names (ADR 0012).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Lineups {
    pub home: BTreeMap<Slot, Vec<String>>,
    pub away: BTreeMap<Slot, Vec<String>>,
}

impl Lineups {
    pub fn get(&self, side: Side, slot: Slot) -> &[String] {
        let map = match side {
            Side::Home => &self.home,
            Side::Away => &self.away,
        };
        map.get(&slot).map(Vec::as_slice).unwrap_or_default()
    }

    fn side_mut(&mut self, side: Side) -> &mut BTreeMap<Slot, Vec<String>> {
        match side {
            Side::Home => &mut self.home,
            Side::Away => &mut self.away,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum CommandError {
    #[error("stale revision {got}, current is {current}")]
    Conflict { current: u64, got: u64 },
    #[error("the match is over")]
    Finished,
    #[error("nothing to undo")]
    NothingToUndo,
    #[error("unit {0} does not exist")]
    NoSuchUnit(usize),
    #[error("unit {0} is already played")]
    UnitDone(usize),
    #[error("units cannot be selected in a race")]
    SelectInRace,
    #[error("slot {0} is not part of this match day")]
    NoSuchSlot(Slot),
    #[error("slot {slot} takes at most {max} players")]
    TooManyPlayers { slot: Slot, max: usize },
    #[error("the format of this match comes from its league")]
    FormatFixed,
    #[error("sets are only entered by hand in a set format without rules")]
    SetsNotManual,
    #[error("the game is over; continue with the next one")]
    GameOver,
    #[error("only the set finished last can be taken back")]
    NotLastSet,
    #[error("the match is not a race")]
    NotARace,
    #[error("a match day needs at least one unit")]
    NoUnits,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TableMatch {
    pub labels: Labels,
    pub home: Team,
    pub away: Team,
    pub format: Format,
    pub units: Vec<UnitSpec>,
    pub lineups: Lineups,
    pub competition: Competition,
    events: Vec<Event>,
    revision: u64,
}

/// Derived state after replaying the event log.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Progress {
    /// Goals per unit, home and away.
    pub scores: Vec<(u16, u16)>,
    /// Whether each unit is over (played, or skipped in a decided best-of game).
    pub done: Vec<bool>,
    /// The unit goals go to. After the match, the last unit played.
    pub current: usize,
    pub outcome: Option<Outcome>,
    /// Sets won, entered by hand (single games in sets).
    pub sets: (u16, u16),
    /// Who won each unit (set), entered by hand (MTFV set leagues).
    pub winners: Vec<Option<Side>>,
}

impl Progress {
    pub fn totals(&self) -> (u16, u16) {
        totals(&self.scores)
    }
}

fn totals(scores: &[(u16, u16)]) -> (u16, u16) {
    scores
        .iter()
        .fold((0, 0), |(h, a), (uh, ua)| (h + uh, a + ua))
}

fn add_goal(score: &mut (u16, u16), side: Side) {
    match side {
        Side::Home => score.0 += 1,
        Side::Away => score.1 += 1,
    }
}

impl TableMatch {
    pub fn new(
        labels: Labels,
        home: Team,
        away: Team,
        format: Format,
        units: Vec<UnitSpec>,
        competition: Competition,
    ) -> Result<Self, CommandError> {
        if units.is_empty() {
            return Err(CommandError::NoUnits);
        }
        Ok(Self {
            labels,
            home,
            away,
            format,
            units,
            lineups: Lineups::default(),
            competition,
            events: Vec::new(),
            revision: 0,
        })
    }

    /// A Kickertool or Frei match: one game, singles or doubles.
    pub fn single_game(
        competition: Competition,
        labels: Labels,
        home: Team,
        away: Team,
        singles: bool,
        format: MatchFormat,
    ) -> Self {
        let slot = if singles {
            Slot::singles(1)
        } else {
            Slot::doubles(1)
        };
        Self {
            labels,
            home,
            away,
            format: format.format(),
            units: vec![UnitSpec {
                home: slot,
                away: slot,
                flag: None,
            }],
            lineups: Lineups::default(),
            competition,
            events: Vec::new(),
            revision: 0,
        }
    }

    /// The operator's format, for a Kickertool or Frei match.
    pub fn match_format(&self) -> Option<MatchFormat> {
        self.competition
            .is_single_game()
            .then(|| MatchFormat::from_format(self.format))
    }

    /// Changes target and segment length of a race (a setting for the day).
    /// A draw stays one goal before the target. The log is replayed.
    pub fn set_race(
        &mut self,
        target: u16,
        segment: u16,
        expected_revision: u64,
    ) -> Result<(), CommandError> {
        self.check_revision(expected_revision)?;
        let Format::Race { draw_at, .. } = self.format else {
            return Err(CommandError::NotARace);
        };
        let target = target.max(1);
        self.format = Format::Race {
            target,
            segment,
            draw_at: draw_at.map(|_| target - 1),
        };
        self.revision += 1;
        Ok(())
    }

    /// Changes the format of a single game. The event log is kept and
    /// replayed under the new rules.
    pub fn set_format(
        &mut self,
        format: MatchFormat,
        expected_revision: u64,
    ) -> Result<(), CommandError> {
        self.check_revision(expected_revision)?;
        if !self.competition.is_single_game() {
            return Err(CommandError::FormatFixed);
        }
        self.format = format.format();
        self.revision += 1;
        Ok(())
    }

    /// Replaces the labels and team names given; `None` keeps them.
    pub fn set_setup(
        &mut self,
        labels: Option<Labels>,
        home: Option<String>,
        away: Option<String>,
        expected_revision: u64,
    ) -> Result<(), CommandError> {
        self.check_revision(expected_revision)?;
        if let Some(labels) = labels {
            self.labels = labels;
        }
        if let Some(name) = home {
            self.home.name = name;
        }
        if let Some(name) = away {
            self.away.name = name;
        }
        self.revision += 1;
        Ok(())
    }

    /// The slot a side plays in the current unit.
    pub fn current_slot(&self, side: Side) -> Slot {
        self.units[self.progress().current].slot(side)
    }

    /// Changes with every mutation. Clients send it back with each command.
    pub fn revision(&self) -> u64 {
        self.revision
    }

    #[cfg(test)]
    pub fn events(&self) -> &[Event] {
        &self.events
    }

    pub fn team(&self, side: Side) -> &Team {
        match side {
            Side::Home => &self.home,
            Side::Away => &self.away,
        }
    }

    fn check_revision(&self, expected: u64) -> Result<(), CommandError> {
        if expected != self.revision {
            return Err(CommandError::Conflict {
                current: self.revision,
                got: expected,
            });
        }
        Ok(())
    }

    /// Validates and appends an event.
    pub fn apply(&mut self, event: Event, expected_revision: u64) -> Result<(), CommandError> {
        self.check_revision(expected_revision)?;
        let progress = self.progress();
        let sets_league = matches!(self.format, Format::Sets { .. });
        match event {
            Event::SetWon { .. } if matches!(self.format, Format::Race { .. }) => {
                return Err(CommandError::SetsNotManual);
            }
            Event::CorrectSets { .. } if self.format != Format::ManualSets => {
                return Err(CommandError::SetsNotManual);
            }
            Event::Goal { .. } | Event::Correct { .. } | Event::SetWon { .. }
                if sets_league && progress.outcome.is_none() && progress.done[progress.current] =>
            {
                return Err(CommandError::GameOver);
            }
            Event::Goal { .. }
            | Event::Finish
            | Event::Correct { .. }
            | Event::SetWon { .. }
            | Event::CorrectSets { .. }
                if progress.outcome.is_some() =>
            {
                return Err(CommandError::Finished);
            }
            Event::Undo if self.effective_events().is_empty() => {
                return Err(CommandError::NothingToUndo);
            }
            Event::Select { .. } if matches!(self.format, Format::Race { .. }) => {
                return Err(CommandError::SelectInRace);
            }
            Event::Select { .. } if progress.outcome.is_some() => {
                return Err(CommandError::Finished);
            }
            Event::Select { unit } if unit >= self.units.len() => {
                return Err(CommandError::NoSuchUnit(unit));
            }
            Event::Select { unit } if progress.done[unit] => {
                return Err(CommandError::UnitDone(unit));
            }
            _ => {}
        }
        self.events.push(event);
        self.revision += 1;
        Ok(())
    }

    /// Takes back the set finished last, if `side` won it and nothing was
    /// entered since (MTFV set leagues and single games).
    pub fn reopen_set(&mut self, side: Side, expected_revision: u64) -> Result<(), CommandError> {
        self.check_revision(expected_revision)?;
        if self.effective_events().last() != Some(&Event::SetWon { side }) {
            return Err(CommandError::NotLastSet);
        }
        self.events.push(Event::Undo);
        self.revision += 1;
        Ok(())
    }

    /// The first unit of the game after the current one, once the current
    /// game is over (MTFV set leagues).
    pub fn next_game(&self) -> Option<usize> {
        let p = self.progress();
        if p.outcome.is_some() || !p.done[p.current] {
            return None;
        }
        game_groups(&self.units)
            .into_iter()
            .filter(|g| g.start > p.current)
            .map(|g| g.start)
            .find(|&start| !p.done[start])
    }

    /// Sets the players for one side's slot: roster ids or typed names. An
    /// empty list clears it.
    pub fn set_lineup(
        &mut self,
        side: Side,
        slot: Slot,
        players: Vec<String>,
        expected_revision: u64,
    ) -> Result<(), CommandError> {
        self.check_revision(expected_revision)?;
        let player_ids: Vec<String> = players
            .into_iter()
            .map(|p| p.trim().to_owned())
            .filter(|p| !p.is_empty())
            .collect();
        self.validate_lineup(side, slot, &player_ids)?;
        let map = self.lineups.side_mut(side);
        if player_ids.is_empty() {
            map.remove(&slot);
        } else {
            map.insert(slot, player_ids);
        }
        self.revision += 1;
        Ok(())
    }

    fn validate_lineup(
        &self,
        side: Side,
        slot: Slot,
        player_ids: &[String],
    ) -> Result<(), CommandError> {
        if !self.units.iter().any(|u| u.slot(side) == slot) {
            return Err(CommandError::NoSuchSlot(slot));
        }
        let max = slot.kind.players();
        if player_ids.len() > max {
            return Err(CommandError::TooManyPlayers { slot, max });
        }
        Ok(())
    }

    /// Fills a lineup from the source without validation against the roster
    /// and without bumping the revision. Used while building a match day.
    pub(crate) fn prefill_lineup(&mut self, side: Side, slot: Slot, player_ids: Vec<String>) {
        if !player_ids.is_empty() {
            self.lineups
                .side_mut(side)
                .entry(slot)
                .or_insert(player_ids);
        }
    }

    fn effective_events(&self) -> Vec<Event> {
        let mut stack = Vec::with_capacity(self.events.len());
        for event in &self.events {
            match event {
                Event::Undo => {
                    stack.pop();
                }
                other => stack.push(*other),
            }
        }
        stack
    }

    pub fn progress(&self) -> Progress {
        let n = self.units.len();
        let mut p = Progress {
            scores: vec![(0, 0); n],
            done: vec![false; n],
            current: 0,
            outcome: None,
            sets: (0, 0),
            winners: vec![None; n],
        };
        for event in self.effective_events() {
            if p.outcome.is_some() {
                break;
            }
            match event {
                Event::Goal { side } => match self.format {
                    Format::Race { .. } => self.race_goal(&mut p, side),
                    Format::Sets { .. } | Format::ManualSets => {
                        add_goal(&mut p.scores[p.current], side)
                    }
                },
                Event::Select { unit } if unit < n => p.current = unit,
                Event::Select { .. } | Event::Undo => {}
                Event::Finish => p.outcome = Some(self.points_outcome(&p)),
                Event::Correct { home, away } => match self.format {
                    Format::Race { .. } => self.race_correct(&mut p, home, away),
                    Format::Sets { .. } | Format::ManualSets => p.scores[p.current] = (home, away),
                },
                Event::SetWon { side } => match self.format {
                    Format::Sets { .. } => self.set_won(&mut p, side),
                    _ => {
                        add_goal(&mut p.sets, side);
                        p.scores[p.current] = (0, 0);
                    }
                },
                Event::CorrectSets { home, away } => p.sets = (home, away),
            }
        }
        if let Format::Race { .. } = self.format {
            let current = p.current;
            let over = p.outcome.is_some();
            for (i, done) in p.done.iter_mut().enumerate() {
                *done = i < current || (over && i == current);
            }
        }
        p
    }

    fn race_goal(&self, p: &mut Progress, side: Side) {
        let n = self.units.len();
        let (h, a) = p.totals();
        let cur = self.format.race_segment(h, a, n);
        add_goal(&mut p.scores[cur], side);
        let (h, a) = p.totals();
        match self.format.race_outcome(h, a) {
            Some(outcome) => {
                p.current = cur;
                p.outcome = Some(outcome);
            }
            None => p.current = self.format.race_segment(h, a, n),
        }
    }

    /// Replays the corrected totals as alternating goals, so segments and
    /// the outcome follow the same rules as counted goals.
    fn race_correct(&self, p: &mut Progress, home: u16, away: u16) {
        p.scores.iter_mut().for_each(|s| *s = (0, 0));
        p.current = 0;
        let (mut h, mut a) = (0, 0);
        while (h < home || a < away) && p.outcome.is_none() {
            if h < home && (h <= a || a >= away) {
                self.race_goal(p, Side::Home);
                h += 1;
            } else {
                self.race_goal(p, Side::Away);
                a += 1;
            }
        }
    }

    /// A set of an MTFV game ends by hand; its goals stay. The next set of
    /// the same game follows; the next game only by the operator (`select`).
    /// After the last set the match is over.
    fn set_won(&self, p: &mut Progress, side: Side) {
        let cur = p.current;
        p.winners[cur] = Some(side);
        p.done[cur] = true;
        if p.done.iter().all(|&d| d) {
            p.outcome = Some(self.points_outcome(p));
            return;
        }
        let same_game = (cur + 1 < self.units.len())
            && (self.units[cur + 1].home, self.units[cur + 1].away)
                == (self.units[cur].home, self.units[cur].away);
        if same_game && !p.done[cur + 1] {
            p.current = cur + 1;
        }
    }

    fn sets_won(winners: &[Option<Side>]) -> (u16, u16) {
        winners.iter().fold((0, 0), |(h, a), w| match w {
            Some(Side::Home) => (h + 1, a),
            Some(Side::Away) => (h, a + 1),
            None => (h, a),
        })
    }

    /// Points that decide the match: goals in a race, sets won otherwise.
    pub fn match_points(&self, p: &Progress) -> (u16, u16) {
        match self.format {
            Format::Race { .. } => p.totals(),
            Format::Sets { .. } => Self::sets_won(&p.winners),
            Format::ManualSets => p.sets,
        }
    }

    fn points_outcome(&self, p: &Progress) -> Outcome {
        let (h, a) = self.match_points(p);
        Outcome::from_points(h, a)
    }

    /// A lineup entry is a roster id, or else a typed name.
    fn resolve_players(&self, side: Side, slot: Slot) -> Vec<Player> {
        let roster = &self.team(side).roster;
        self.lineups
            .get(side, slot)
            .iter()
            .map(|entry| {
                roster
                    .iter()
                    .find(|p| &p.id == entry)
                    .cloned()
                    .unwrap_or_else(|| Player {
                        id: String::new(),
                        first_name: entry.clone(),
                        last_name: String::new(),
                    })
            })
            .collect()
    }

    /// Everything a client needs to display the current state.
    pub fn scoreboard(&self) -> Scoreboard {
        let p = self.progress();
        let unit = self.units[p.current];
        let (match_home, match_away) = self.match_points(&p);

        let (score, game_sets, set_in_game, sets_in_game) = match self.format {
            Format::Race { .. } => (p.totals(), None, None, None),
            Format::ManualSets => (p.scores[p.current], Some(p.sets), None, None),
            Format::Sets { .. } => {
                let group = game_groups(&self.units)
                    .into_iter()
                    .find(|g| g.contains(&p.current))
                    .unwrap_or(p.current..p.current + 1);
                let won = Self::sets_won(&p.winners[group.clone()]);
                let position = (p.current - group.start + 1) as u16;
                (
                    p.scores[p.current],
                    Some(won),
                    Some(position),
                    Some(group.len() as u16),
                )
            }
        };

        let side_view = |side: Side| {
            let pick = |(h, a): (u16, u16)| if side == Side::Home { h } else { a };
            SideView {
                team: self.team(side).name.clone(),
                score: pick(score),
                match_points: pick((match_home, match_away)),
                game_sets: game_sets.map(pick),
                slot: unit.slot(side),
                players: self.resolve_players(side, unit.slot(side)),
            }
        };

        Scoreboard {
            revision: self.revision,
            competition: self.competition.key(),
            title: self.labels.title(),
            labels: self.labels.clone(),
            format: self.format,
            match_format: self.match_format(),
            segment: {
                let (h, a) = p.totals();
                self.format.race_segment_of(h, a)
            },
            home: side_view(Side::Home),
            away: side_view(Side::Away),
            unit: UnitView {
                index: p.current,
                count: self.units.len(),
                singles: unit.home.kind == SlotKind::Singles,
                set_in_game,
                sets_in_game,
                game: game_groups(&self.units)
                    .iter()
                    .position(|g| g.contains(&p.current))
                    .map_or(1, |i| i + 1),
                games: game_groups(&self.units).len(),
                game_over: p.outcome.is_none()
                    && matches!(self.format, Format::Sets { .. })
                    && p.done[p.current],
            },
            outcome: p.outcome,
            units: self
                .units
                .iter()
                .zip(p.scores.iter().zip(&p.done))
                .map(|(u, (&(home, away), &done))| UnitResult {
                    home_slot: u.home,
                    away_slot: u.away,
                    home,
                    away,
                    done,
                })
                .collect(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Scoreboard {
    pub revision: u64,
    /// `kickertool`, `mtfv` or `manual`.
    pub competition: &'static str,
    /// Tournament and round, joined.
    pub title: String,
    pub labels: Labels,
    /// Set for Kickertool and Frei matches.
    pub match_format: Option<MatchFormat>,
    /// The race segment, 1-based, and how many there are.
    pub segment: Option<(u16, u16)>,
    pub format: Format,
    pub home: SideView,
    pub away: SideView,
    pub unit: UnitView,
    pub outcome: Option<Outcome>,
    pub units: Vec<UnitResult>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SideView {
    pub team: String,
    /// Goals in the current set, or the running total in a race.
    pub score: u16,
    /// Sets won over the match day, or the running total in a race.
    pub match_points: u16,
    /// Sets won in the current game. Absent in a race.
    pub game_sets: Option<u16>,
    pub slot: Slot,
    pub players: Vec<Player>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct UnitView {
    pub index: usize,
    pub count: usize,
    pub singles: bool,
    /// 1-based set number within the current game. Absent in a race.
    pub set_in_game: Option<u16>,
    pub sets_in_game: Option<u16>,
    /// 1-based game (a run of units of one slot pair), and how many.
    pub game: usize,
    pub games: usize,
    /// The current game's sets are all entered; the operator moves on.
    pub game_over: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct UnitResult {
    pub home_slot: Slot,
    pub away_slot: Slot,
    pub home: u16,
    pub away: u16,
    pub done: bool,
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::competition::parse_modus;

    pub fn team(id: &str, players: &[(&str, &str)]) -> Team {
        Team {
            id: id.into(),
            name: format!("Team {id}"),
            roster: players
                .iter()
                .map(|(pid, name)| Player {
                    id: (*pid).into(),
                    first_name: (*name).into(),
                    last_name: "X".into(),
                })
                .collect(),
        }
    }

    fn day(format: Format, modus: &str) -> TableMatch {
        TableMatch::new(
            Labels {
                tournament: "Test".into(),
                ..Default::default()
            },
            team("h", &[("1", "A"), ("2", "B")]),
            team("a", &[("3", "C"), ("4", "D")]),
            format,
            parse_modus(modus).unwrap(),
            Competition::Manual,
        )
        .unwrap()
    }

    pub fn goal(m: &mut TableMatch, side: Side) {
        let rev = m.revision();
        m.apply(Event::Goal { side }, rev).unwrap();
    }

    /// Plays one unit so that it ends exactly at (home, away): the side that
    /// ends lower (running total in a race, unit score otherwise) scores
    /// first, so the unit ends on the other side's last goal.
    pub fn play_unit(m: &mut TableMatch, home: u16, away: u16) {
        let (base_h, base_a) = match m.format {
            Format::Race { .. } => m.progress().totals(),
            Format::Sets { .. } | Format::ManualSets => (0, 0),
        };
        let order = if base_h + home >= base_a + away {
            [(Side::Away, away), (Side::Home, home)]
        } else {
            [(Side::Home, home), (Side::Away, away)]
        };
        for (side, n) in order {
            for _ in 0..n {
                goal(m, side);
            }
        }
    }

    #[test]
    fn race_from_adr_example() {
        let mut m = day(Format::mtfv_race(), "D1D1,D2D2,E1E1,D3D3,E2E2,D4D4,D5D5");
        // running totals 6:3 12:10 18:13 24:15 30:23 35:36 42:40
        for (h, a) in [(6, 3), (6, 7), (6, 3), (6, 2), (6, 8), (5, 13), (7, 4)] {
            play_unit(&mut m, h, a);
        }
        let p = m.progress();
        assert_eq!(p.totals(), (42, 40));
        assert_eq!(p.outcome, Some(Outcome::Home));
        assert_eq!(p.scores[5], (5, 13));
        assert!(p.done.iter().all(|d| *d));
    }

    #[test]
    fn race_segment_advances_on_leader_threshold() {
        let mut m = day(Format::mtfv_race(), "D1D1,D2D2,E1E1,D3D3,E2E2,D4D4,D5D5");
        for _ in 0..5 {
            goal(&mut m, Side::Home);
        }
        assert_eq!(m.progress().current, 0);
        goal(&mut m, Side::Home);
        let sb = m.scoreboard();
        assert_eq!(sb.unit.index, 1);
        assert_eq!(sb.home.score, 6);
        assert_eq!(sb.home.slot, Slot::doubles(2));
        assert_eq!(sb.home.game_sets, None);
    }

    #[test]
    fn race_draw_at_41() {
        let mut m = day(Format::mtfv_race(), "D1D1,D2D2,E1E1,D3D3,E2E2,D4D4,D5D5");
        for (h, a) in [(6, 5), (5, 7), (7, 3), (6, 3), (6, 7), (6, 8), (5, 8)] {
            play_unit(&mut m, h, a);
        }
        let p = m.progress();
        assert_eq!(p.totals(), (41, 41));
        assert_eq!(p.outcome, Some(Outcome::Draw));
        let rev = m.revision();
        assert_eq!(
            m.apply(Event::Goal { side: Side::Home }, rev),
            Err(CommandError::Finished)
        );
    }

    #[test]
    fn undo_crosses_segment_boundary() {
        let mut m = day(Format::mtfv_race(), "D1D1,D2D2,E1E1,D3D3,E2E2,D4D4,D5D5");
        for _ in 0..6 {
            goal(&mut m, Side::Away);
        }
        assert_eq!(m.progress().current, 1);
        let rev = m.revision();
        m.apply(Event::Undo, rev).unwrap();
        let p = m.progress();
        assert_eq!(p.current, 0);
        assert_eq!(p.scores[0], (0, 5));
        assert!(!p.done[0]);
    }

    #[test]
    fn undo_reverts_finish_and_double_undo() {
        let mut m = day(Format::mtfv_sets(), "D1D1H,D1D1G");
        goal(&mut m, Side::Home);
        goal(&mut m, Side::Home);
        let rev = m.revision();
        m.apply(Event::Finish, rev).unwrap();
        assert_eq!(m.progress().outcome, Some(Outcome::Draw));
        m.apply(Event::Undo, m.revision()).unwrap();
        assert_eq!(m.progress().outcome, None);
        m.apply(Event::Undo, m.revision()).unwrap();
        assert_eq!(m.progress().scores[0], (1, 0));
        m.apply(Event::Undo, m.revision()).unwrap();
        assert_eq!(
            m.apply(Event::Undo, m.revision()),
            Err(CommandError::NothingToUndo)
        );
    }

    #[test]
    fn stale_revision_is_rejected() {
        let mut m = day(Format::mtfv_sets(), "D1D1H,D1D1G");
        goal(&mut m, Side::Home);
        assert_eq!(
            m.apply(Event::Goal { side: Side::Home }, 0),
            Err(CommandError::Conflict { current: 1, got: 0 })
        );
        assert_eq!(m.progress().scores[0], (1, 0));
    }

    fn won(m: &mut TableMatch, side: Side) {
        let rev = m.revision();
        m.apply(Event::SetWon { side }, rev).unwrap();
    }

    fn next_game(m: &mut TableMatch) {
        let unit = m.next_game().expect("a next game");
        let rev = m.revision();
        m.apply(Event::Select { unit }, rev).unwrap();
    }

    #[test]
    fn mtfv_sets_end_by_hand_and_keep_their_goals() {
        let mut m = day(Format::mtfv_sets(), "D1D1H,D1D1G,E1E1H,E1E1G");
        play_unit(&mut m, 8, 7);
        let sb = m.scoreboard();
        assert_eq!(sb.outcome, None);
        assert_eq!(sb.unit.set_in_game, Some(1), "goals never end a set");
        won(&mut m, Side::Home);
        let sb = m.scoreboard();
        assert_eq!(sb.unit.set_in_game, Some(2), "the game's next set follows");
        assert_eq!((sb.home.score, sb.away.score), (0, 0));
        assert_eq!(sb.home.game_sets, Some(1));
        assert_eq!(m.progress().scores[0], (8, 7), "the set keeps its goals");
    }

    #[test]
    fn the_operator_moves_on_to_the_next_game() {
        let mut m = day(Format::mtfv_sets(), "D1D1H,D1D1G,E1E1H,E1E1G");
        won(&mut m, Side::Home);
        assert_eq!(m.next_game(), None, "the game is not over");
        won(&mut m, Side::Away);
        let sb = m.scoreboard();
        assert!(sb.unit.game_over);
        assert_eq!((sb.home.game_sets, sb.away.game_sets), (Some(1), Some(1)));
        let rev = m.revision();
        assert_eq!(
            m.apply(Event::Goal { side: Side::Home }, rev),
            Err(CommandError::GameOver)
        );
        assert_eq!(m.next_game(), Some(2));
        next_game(&mut m);
        let sb = m.scoreboard();
        assert!(!sb.unit.game_over);
        assert_eq!((sb.unit.game, sb.unit.games), (2, 2));
        assert_eq!(sb.unit.set_in_game, Some(1));
        assert_eq!(sb.home.game_sets, Some(0));
        assert_eq!((sb.home.match_points, sb.away.match_points), (1, 1));
    }

    #[test]
    fn the_last_set_ends_the_begegnung_by_sets() {
        let mut m = day(Format::mtfv_sets(), "D1D1H,D1D1G,E1E1H,E1E1G");
        won(&mut m, Side::Home);
        won(&mut m, Side::Home);
        next_game(&mut m);
        won(&mut m, Side::Away);
        won(&mut m, Side::Home);
        let sb = m.scoreboard();
        assert_eq!(sb.outcome, Some(Outcome::Home));
        assert_eq!((sb.home.match_points, sb.away.match_points), (3, 1));
        assert!(!sb.unit.game_over);
    }

    #[test]
    fn only_the_last_set_can_be_reopened() {
        let mut m = day(Format::mtfv_sets(), "D1D1H,D1D1G,E1E1H,E1E1G");
        goal(&mut m, Side::Home);
        won(&mut m, Side::Home);
        let rev = m.revision();
        assert_eq!(m.reopen_set(Side::Away, rev), Err(CommandError::NotLastSet));
        m.reopen_set(Side::Home, rev).unwrap();
        let sb = m.scoreboard();
        assert_eq!(sb.unit.set_in_game, Some(1));
        assert_eq!(sb.home.score, 1);
        assert_eq!(sb.home.game_sets, Some(0));

        won(&mut m, Side::Home);
        goal(&mut m, Side::Away);
        let rev = m.revision();
        assert_eq!(
            m.reopen_set(Side::Home, rev),
            Err(CommandError::NotLastSet),
            "a goal came after it"
        );
    }

    #[test]
    fn select_jumps_to_unit_in_sets_only() {
        let mut m = day(Format::mtfv_sets(), "D1D1H,D1D1G,E1E1H,E1E1G");
        m.apply(Event::Select { unit: 2 }, 0).unwrap();
        goal(&mut m, Side::Away);
        assert_eq!(m.progress().scores[2], (0, 1));
        assert_eq!(
            m.apply(Event::Select { unit: 9 }, m.revision()),
            Err(CommandError::NoSuchUnit(9))
        );

        let mut race = day(Format::mtfv_race(), "D1D1,D2D2");
        assert_eq!(
            race.apply(Event::Select { unit: 1 }, 0),
            Err(CommandError::SelectInRace)
        );
    }

    #[test]
    fn lineups_are_validated_and_resolved() {
        let mut m = day(Format::mtfv_sets(), "D1D1H,D1D1G,E1E1H,E1E1G");
        m.set_lineup(
            Side::Home,
            Slot::doubles(1),
            vec!["1".into(), "2".into()],
            0,
        )
        .unwrap();
        assert_eq!(m.revision(), 1);
        assert_eq!(
            m.set_lineup(
                Side::Home,
                Slot::singles(1),
                vec!["1".into(), "2".into()],
                1
            ),
            Err(CommandError::TooManyPlayers {
                slot: Slot::singles(1),
                max: 1
            })
        );
        assert_eq!(
            m.set_lineup(Side::Away, Slot::doubles(3), vec![], 1),
            Err(CommandError::NoSuchSlot(Slot::doubles(3)))
        );
        let sb = m.scoreboard();
        let names: Vec<_> = sb.home.players.iter().map(Player::display_name).collect();
        assert_eq!(names, vec!["A X", "B X"]);
        assert!(sb.away.players.is_empty());
    }

    #[test]
    fn persists_and_restores() {
        let mut m = day(Format::mtfv_race(), "D1D1,D2D2,E1E1,D3D3,E2E2,D4D4,D5D5");
        play_unit(&mut m, 6, 2);
        let json = serde_json::to_string(&m).unwrap();
        let back: TableMatch = serde_json::from_str(&json).unwrap();
        assert_eq!(back, m);
        assert_eq!(back.progress().totals(), (6, 2));
    }

    fn single(format: MatchFormat) -> TableMatch {
        TableMatch::single_game(
            Competition::Manual,
            Labels::default(),
            Team::named("A"),
            Team::named("B"),
            false,
            format,
        )
    }

    fn correct(m: &mut TableMatch, home: u16, away: u16) -> Result<(), CommandError> {
        let rev = m.revision();
        m.apply(Event::Correct { home, away }, rev)
    }

    fn set_won(m: &mut TableMatch, side: Side) {
        let rev = m.revision();
        m.apply(Event::SetWon { side }, rev).unwrap();
    }

    #[test]
    fn manual_sets_never_end_by_goals() {
        let mut m = single(MatchFormat::Sets {});
        assert_eq!(m.units.len(), 1);
        for _ in 0..12 {
            goal(&mut m, Side::Home);
        }
        let sb = m.scoreboard();
        assert_eq!(sb.home.score, 12, "overtime: no set ends by itself");
        assert_eq!(sb.home.game_sets, Some(0));
        assert_eq!(sb.outcome, None);
        assert_eq!(sb.unit.set_in_game, None);
        assert_eq!(sb.match_format, Some(MatchFormat::Sets {}));
    }

    #[test]
    fn a_set_entered_by_hand_starts_the_next_at_zero() {
        let mut m = single(MatchFormat::Sets {});
        correct(&mut m, 5, 3).unwrap();
        set_won(&mut m, Side::Home);
        let sb = m.scoreboard();
        assert_eq!((sb.home.score, sb.away.score), (0, 0));
        assert_eq!((sb.home.game_sets, sb.away.game_sets), (Some(1), Some(0)));
        assert_eq!((sb.home.match_points, sb.away.match_points), (1, 0));

        let rev = m.revision();
        m.apply(Event::CorrectSets { home: 1, away: 1 }, rev)
            .unwrap();
        assert_eq!(m.scoreboard().away.game_sets, Some(1));

        let rev = m.revision();
        m.apply(Event::Undo, rev).unwrap();
        let rev = m.revision();
        m.apply(Event::Undo, rev).unwrap();
        let sb = m.scoreboard();
        assert_eq!(
            (sb.home.score, sb.away.score),
            (5, 3),
            "undo brings the set back"
        );
        assert_eq!(sb.home.game_sets, Some(0));
    }

    #[test]
    fn finish_decides_by_sets() {
        let mut m = single(MatchFormat::Sets {});
        set_won(&mut m, Side::Away);
        goal(&mut m, Side::Home);
        let rev = m.revision();
        m.apply(Event::Finish, rev).unwrap();
        assert_eq!(m.scoreboard().outcome, Some(Outcome::Away));
        let rev = m.revision();
        assert_eq!(
            m.apply(Event::SetWon { side: Side::Home }, rev),
            Err(CommandError::Finished)
        );
    }

    #[test]
    fn sets_are_only_entered_by_hand_in_manual_sets() {
        let mut race = single(MatchFormat::Race {
            target: 10,
            segment: 0,
        });
        assert_eq!(
            race.apply(Event::SetWon { side: Side::Home }, 0),
            Err(CommandError::SetsNotManual)
        );
        assert_eq!(
            race.apply(Event::CorrectSets { home: 1, away: 0 }, 0),
            Err(CommandError::SetsNotManual)
        );
        assert_eq!(race.scoreboard().home.game_sets, None);
    }

    #[test]
    fn correct_in_a_race_sets_the_totals() {
        let mut m = day(Format::mtfv_race(), "D1D1,D2D2,E1E1,D3D3,E2E2,D4D4,D5D5");
        correct(&mut m, 14, 9).unwrap();
        let sb = m.scoreboard();
        assert_eq!((sb.home.score, sb.away.score), (14, 9));
        assert_eq!(sb.unit.index, 2, "segments follow the totals");
        correct(&mut m, 42, 30).unwrap();
        assert_eq!(m.scoreboard().outcome, Some(Outcome::Home));
        assert_eq!(correct(&mut m, 1, 1), Err(CommandError::Finished));
    }

    #[test]
    fn format_change_replays_the_log() {
        let mut m = single(MatchFormat::Race {
            target: 10,
            segment: 0,
        });
        correct(&mut m, 4, 2).unwrap();
        assert_eq!(m.scoreboard().outcome, None);
        let rev = m.revision();
        m.set_format(
            MatchFormat::Race {
                target: 4,
                segment: 0,
            },
            rev,
        )
        .unwrap();
        assert_eq!(m.scoreboard().outcome, Some(Outcome::Home));
        let rev = m.revision();
        m.set_format(MatchFormat::Sets {}, rev).unwrap();
        let sb = m.scoreboard();
        assert_eq!(sb.outcome, None);
        assert_eq!((sb.home.score, sb.away.score), (4, 2));
    }

    #[test]
    fn mtfv_format_is_fixed() {
        let mut m = day(Format::mtfv_race(), "D1D1,D2D2");
        m.competition = Competition::Mtfv {
            veranstaltung_id: 1,
            begegnung_id: 2,
            teamspiel_ids: vec![],
        };
        assert_eq!(m.match_format(), None);
        assert_eq!(
            m.set_format(
                MatchFormat::Race {
                    target: 4,
                    segment: 0,
                },
                0
            ),
            Err(CommandError::FormatFixed)
        );
    }

    #[test]
    fn setup_changes_names_and_labels() {
        let mut m = single(MatchFormat::Sets {});
        let labels = Labels {
            tournament: "Cup".into(),
            round: "Finale".into(),
            ..Default::default()
        };
        m.set_setup(Some(labels), None, Some("Gast".into()), 0)
            .unwrap();
        let sb = m.scoreboard();
        assert_eq!(sb.title, "Cup · Finale");
        assert_eq!(sb.home.team, "A");
        assert_eq!(sb.away.team, "Gast");
        assert_eq!(m.revision(), 1);
    }

    #[test]
    fn typed_names_and_roster_ids_both_resolve() {
        let mut m = day(Format::mtfv_race(), "D1D1,D2D2");
        m.set_lineup(
            Side::Home,
            Slot::doubles(1),
            vec!["1".into(), " Max ".into(), "".into()],
            0,
        )
        .unwrap();
        let names: Vec<_> = m
            .scoreboard()
            .home
            .players
            .iter()
            .map(Player::display_name)
            .collect();
        assert_eq!(names, ["A X", "Max"]);
    }

    #[test]
    fn race_target_changes_and_replays() {
        let mut m = day(Format::mtfv_race(), "D1D1,D2D2");
        correct(&mut m, 30, 20).unwrap();
        let rev = m.revision();
        m.set_race(30, 6, rev).unwrap();
        assert_eq!(
            m.format,
            Format::Race {
                target: 30,
                segment: 6,
                draw_at: Some(29)
            }
        );
        assert_eq!(m.scoreboard().outcome, Some(Outcome::Home));
        let mut sets = single(MatchFormat::Sets {});
        assert_eq!(sets.set_race(30, 6, 0), Err(CommandError::NotARace));
    }
}
