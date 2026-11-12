//! A match day with lineups and an append-only event log (ADR 0004, 0006).
//!
//! Scores are never stored directly. They are derived by replaying the event
//! log through the format rules, so undo works across set and segment
//! boundaries and a restart only needs the log.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

use super::modus::{SlotKind, game_groups};
use super::{Format, Outcome, SetCount, Side, Slot, UnitSpec};

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
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Source {
    Manual,
    Mtfv {
        veranstaltung_id: u32,
        begegnung_id: u32,
        /// `teamspiel_id` per unit, in unit order.
        teamspiel_ids: Vec<u64>,
    },
}

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
    #[error("player {0} is not on the roster")]
    UnknownPlayer(String),
    #[error("a match day needs at least one unit")]
    NoUnits,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MatchDay {
    pub title: String,
    pub home: Team,
    pub away: Team,
    pub format: Format,
    pub units: Vec<UnitSpec>,
    pub lineups: Lineups,
    pub source: Source,
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

impl MatchDay {
    pub fn new(
        title: String,
        home: Team,
        away: Team,
        format: Format,
        units: Vec<UnitSpec>,
        source: Source,
    ) -> Result<Self, CommandError> {
        if units.is_empty() {
            return Err(CommandError::NoUnits);
        }
        Ok(Self {
            title,
            home,
            away,
            format,
            units,
            lineups: Lineups::default(),
            source,
            events: Vec::new(),
            revision: 0,
        })
    }

    /// Changes with every mutation. Clients send it back with each command.
    pub fn revision(&self) -> u64 {
        self.revision
    }

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
        match event {
            Event::Goal { .. } | Event::Finish if progress.outcome.is_some() => {
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

    /// Sets the players for one side's slot. An empty list clears it.
    pub fn set_lineup(
        &mut self,
        side: Side,
        slot: Slot,
        player_ids: Vec<String>,
        expected_revision: u64,
    ) -> Result<(), CommandError> {
        self.check_revision(expected_revision)?;
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
        let roster = &self.team(side).roster;
        if let Some(unknown) = player_ids
            .iter()
            .find(|id| !roster.iter().any(|p| &p.id == *id))
        {
            return Err(CommandError::UnknownPlayer(unknown.clone()));
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
        };
        for event in self.effective_events() {
            if p.outcome.is_some() {
                break;
            }
            match event {
                Event::Goal { side } => match self.format {
                    Format::Race { .. } => self.race_goal(&mut p, side),
                    Format::Sets { .. } => self.sets_goal(&mut p, side),
                },
                Event::Select { unit } if unit < n => p.current = unit,
                Event::Select { .. } | Event::Undo => {}
                Event::Finish => p.outcome = Some(self.points_outcome(&p)),
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

    fn sets_goal(&self, p: &mut Progress, side: Side) {
        let cur = p.current;
        add_goal(&mut p.scores[cur], side);
        let (h, a) = p.scores[cur];
        if self.format.set_winner(h, a).is_none() {
            return;
        }
        p.done[cur] = true;
        self.skip_decided_game(p, cur);
        let n = self.units.len();
        let next = (cur + 1..n).chain(0..cur).find(|&i| !p.done[i]);
        match next {
            Some(i) => p.current = i,
            None => p.outcome = Some(self.points_outcome(p)),
        }
    }

    fn skip_decided_game(&self, p: &mut Progress, unit: usize) {
        let Format::Sets {
            sets: SetCount::BestOf(n),
            ..
        } = self.format
        else {
            return;
        };
        let Some(group) = game_groups(&self.units)
            .into_iter()
            .find(|g| g.contains(&unit))
        else {
            return;
        };
        let (h, a) = self.sets_won(&p.scores[group.clone()]);
        if h.max(a) > n / 2 {
            for i in group {
                p.done[i] = true;
            }
        }
    }

    fn sets_won(&self, scores: &[(u16, u16)]) -> (u16, u16) {
        scores.iter().fold((0, 0), |(h, a), &(uh, ua)| {
            match self.format.set_winner(uh, ua) {
                Some(Side::Home) => (h + 1, a),
                Some(Side::Away) => (h, a + 1),
                None => (h, a),
            }
        })
    }

    /// Points that decide the match: goals in a race, sets won otherwise.
    pub fn match_points(&self, p: &Progress) -> (u16, u16) {
        match self.format {
            Format::Race { .. } => p.totals(),
            Format::Sets { .. } => self.sets_won(&p.scores),
        }
    }

    fn points_outcome(&self, p: &Progress) -> Outcome {
        let (h, a) = self.match_points(p);
        Outcome::from_points(h, a)
    }

    fn resolve_players(&self, side: Side, slot: Slot) -> Vec<Player> {
        let roster = &self.team(side).roster;
        self.lineups
            .get(side, slot)
            .iter()
            .map(|id| {
                roster
                    .iter()
                    .find(|p| &p.id == id)
                    .cloned()
                    .unwrap_or_else(|| Player {
                        id: id.clone(),
                        first_name: String::new(),
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
            Format::Sets { .. } => {
                let group = game_groups(&self.units)
                    .into_iter()
                    .find(|g| g.contains(&p.current))
                    .unwrap_or(p.current..p.current + 1);
                let won = self.sets_won(&p.scores[group.clone()]);
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
            title: self.title.clone(),
            format: self.format,
            home: side_view(Side::Home),
            away: side_view(Side::Away),
            unit: UnitView {
                index: p.current,
                count: self.units.len(),
                singles: unit.home.kind == SlotKind::Singles,
                set_in_game,
                sets_in_game,
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
    pub title: String,
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

    fn day(format: Format, modus: &str) -> MatchDay {
        MatchDay::new(
            "Test".into(),
            team("h", &[("1", "A"), ("2", "B")]),
            team("a", &[("3", "C"), ("4", "D")]),
            format,
            parse_modus(modus).unwrap(),
            Source::Manual,
        )
        .unwrap()
    }

    pub fn goal(m: &mut MatchDay, side: Side) {
        let rev = m.revision();
        m.apply(Event::Goal { side }, rev).unwrap();
    }

    /// Plays one unit so that it ends exactly at (home, away): the side that
    /// ends lower (running total in a race, unit score otherwise) scores
    /// first, so the unit ends on the other side's last goal.
    pub fn play_unit(m: &mut MatchDay, home: u16, away: u16) {
        let (base_h, base_a) = match m.format {
            Format::Race { .. } => m.progress().totals(),
            Format::Sets { .. } => (0, 0),
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

    #[test]
    fn fixed_sets_play_every_set() {
        let mut m = day(Format::mtfv_sets(), "D1D1H,D1D1G,E1E1H,E1E1G");
        play_unit(&mut m, 5, 0);
        let sb = m.scoreboard();
        assert_eq!(sb.unit.index, 1);
        assert_eq!(sb.unit.set_in_game, Some(2));
        assert_eq!(sb.home.game_sets, Some(1));
        assert_eq!(sb.home.score, 0);
        play_unit(&mut m, 5, 3);
        // game D1 ended 2:0, second set still played under Fixed(2)
        play_unit(&mut m, 3, 5);
        play_unit(&mut m, 6, 4);
        let p = m.progress();
        assert_eq!(m.match_points(&p), (3, 1));
        assert_eq!(p.outcome, Some(Outcome::Home));
    }

    #[test]
    fn best_of_skips_decided_set() {
        let format = Format::Sets {
            sets: SetCount::BestOf(3),
            points: 5,
            win_by: 1,
            cap: None,
        };
        let mut m = day(format, "D1D1,D1D1,D1D1");
        play_unit(&mut m, 5, 2);
        play_unit(&mut m, 5, 4);
        let p = m.progress();
        assert_eq!(p.outcome, Some(Outcome::Home));
        assert_eq!(p.done, vec![true, true, true]);
        assert_eq!(p.scores[2], (0, 0));
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
            m.set_lineup(Side::Away, Slot::singles(1), vec!["1".into()], 1),
            Err(CommandError::UnknownPlayer("1".into()))
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
        let back: MatchDay = serde_json::from_str(&json).unwrap();
        assert_eq!(back, m);
        assert_eq!(back.progress().totals(), (6, 2));
    }
}
