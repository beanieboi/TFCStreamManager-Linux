//! MTFV league source (ADR 0012): the season overview
//! (`view=sportsmanager`) lists every league of the current season with its
//! format and all Begegnungen, played or not. A match is built from it alone;
//! `begegnung_spielplan` only exists once results are entered.

// Wired into the web server and UI in the next step.
#![allow(dead_code, unused_imports)]

mod client;
mod models;

pub use client::MtfvClient;
pub use models::{BegegnungDetail, BegegnungSummary, Modus, SeasonLeague};

use chrono::{NaiveDate, NaiveDateTime};

use crate::competition::{
    CommandError, Competition, Format, Labels, ModusError, Player, SetCount, Side, TableMatch,
    Team, UnitSpec, game_groups, parse_modus,
};
use models::{Spiel, Spieler, TeamInfo};

/// Format parameters that are not in the mtfv.de data (ADR 0005).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FormatRules {
    pub race_target: u16,
    /// Goals per race segment; `None` takes the league's.
    pub race_segment: Option<u16>,
    pub race_draw_at: Option<u16>,
    pub set_win_by: u16,
    pub set_cap: Option<u16>,
}

impl FormatRules {
    /// The defaults with another race target and segment length; a draw is
    /// one goal before the target.
    pub fn race_to(target: u16, segment: u16) -> Self {
        Self {
            race_target: target,
            race_segment: Some(segment),
            race_draw_at: Some(target.saturating_sub(1)),
            ..Self::default()
        }
    }
}

impl Default for FormatRules {
    fn default() -> Self {
        Self {
            race_target: 42,
            race_segment: None,
            race_draw_at: Some(41),
            set_win_by: 2,
            // 8:7 is the most a set can end; sets are entered by hand, so
            // this is information only.
            set_cap: Some(8),
        }
    }
}

/// Derives the format from a league's `modus`: `punktetyp` 1 with a segment
/// length is a race, everything else is sets with one game per run of
/// identical slot pairs.
pub fn format_for(modus: &Modus, units: &[UnitSpec], rules: &FormatRules) -> Format {
    if modus.punktetyp == 1 && modus.spielpunkte_bedingung > 0 {
        return Format::Race {
            target: rules.race_target,
            segment: rules
                .race_segment
                .unwrap_or(modus.spielpunkte_bedingung as u16),
            draw_at: rules.race_draw_at,
        };
    }
    let sets = game_groups(units)
        .iter()
        .map(|g| g.len() as u16)
        .max()
        .unwrap_or(1);
    let points = modus
        .punkte_sieg_doppel
        .max(modus.punkte_sieg_einzel)
        .try_into()
        .ok()
        .filter(|p: &u16| *p > 0)
        .unwrap_or(5);
    Format::Sets {
        sets: SetCount::Fixed(sets),
        points,
        win_by: rules.set_win_by,
        cap: rules.set_cap,
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ConvertError {
    #[error(transparent)]
    Modus(#[from] ModusError),
    #[error(transparent)]
    TableMatch(#[from] CommandError),
    #[error("Begegnung {0} is not in the league")]
    NoSuchBegegnung(u32),
}

impl SeasonLeague {
    pub fn id(&self) -> u32 {
        self.veranstaltung.veranstaltung_id
    }

    pub fn name(&self) -> &str {
        &self.veranstaltung.bezeichnung
    }

    /// Race (`punktetyp` 1 with a segment length) or sets.
    pub fn is_race(&self) -> bool {
        let modus = &self.tabelle.modus;
        modus.punktetyp == 1 && modus.spielpunkte_bedingung > 0
    }

    /// The games of a Begegnung, e.g. `["D1", "E1", "D2", "E2", "D3"]`, and
    /// how many sets each has.
    pub fn games(&self) -> Vec<(String, usize)> {
        let Ok(units) = parse_modus(&self.tabelle.modus.modus) else {
            return Vec::new();
        };
        game_groups(&units)
            .into_iter()
            .map(|g| (units[g.start].home.to_string(), g.len()))
            .collect()
    }

    /// Every Begegnung of the league, by time.
    pub fn fixtures(&self) -> Vec<Fixture> {
        let mut fixtures: Vec<Fixture> = self
            .begegnungen
            .iter()
            .filter_map(|b| {
                Some(Fixture {
                    veranstaltung_id: self.id(),
                    league: self.name().to_owned(),
                    begegnung_id: b.begegnung_id,
                    time: b.time()?,
                    spieltag: b.spieltag.clone(),
                    home: b.heim_name.clone(),
                    away: b.gast_name.clone(),
                    venue: b.spielort_name.clone().filter(|v| !v.is_empty()),
                })
            })
            .collect();
        fixtures.sort_by_key(|f| f.time);
        fixtures
    }

    /// A Begegnung as the match on the table: league, Spieltag and team
    /// names from the overview, the games and format from the league.
    /// Players and scores are entered live.
    pub fn table_match(
        &self,
        begegnung_id: u32,
        rules: &FormatRules,
    ) -> Result<TableMatch, ConvertError> {
        let begegnung = self
            .begegnungen
            .iter()
            .find(|b| b.begegnung_id == begegnung_id)
            .ok_or(ConvertError::NoSuchBegegnung(begegnung_id))?;
        let units = parse_modus(&self.tabelle.modus.modus)?;
        let format = format_for(&self.tabelle.modus, &units, rules);
        let team = |id: u32, name: &str| Team {
            id: id.to_string(),
            name: name.to_owned(),
            roster: Vec::new(),
        };
        Ok(TableMatch::new(
            Labels {
                tournament: self.name().to_owned(),
                round: begegnung.spieltag.clone(),
                ..Default::default()
            },
            team(begegnung.heim_team_id, &begegnung.heim_name),
            team(begegnung.gast_team_id, &begegnung.gast_name),
            format,
            units,
            Competition::Mtfv {
                veranstaltung_id: self.id(),
                begegnung_id,
                teamspiel_ids: Vec::new(),
            },
        )?)
    }
}

fn team(info: &TeamInfo, roster: &[Spieler]) -> Team {
    let mut players: Vec<Player> = roster
        .iter()
        .map(|s| Player {
            id: s.spieler_id.to_string(),
            first_name: s.vorname.clone(),
            last_name: s.nachname.clone(),
        })
        .collect();
    players.sort_by(|a, b| (&a.last_name, &a.first_name).cmp(&(&b.last_name, &b.first_name)));
    Team {
        id: info.team_id.to_string(),
        name: info.teamname.clone(),
        roster: players,
    }
}

fn spiel_players(spiel: &Spiel, side: Side) -> Vec<String> {
    let ids = match side {
        Side::Home => [spiel.heim_spieler_1_id, spiel.heim_spieler_2_id],
        Side::Away => [spiel.gast_spieler_1_id, spiel.gast_spieler_2_id],
    };
    ids.into_iter().flatten().map(|id| id.to_string()).collect()
}

impl BegegnungDetail {
    /// Builds a match day. Lineups already entered on mtfv.de are copied.
    pub fn to_table_match(&self, rules: &FormatRules) -> Result<TableMatch, ConvertError> {
        let units = parse_modus(&self.modus.modus)?;
        let format = format_for(&self.modus, &units, rules);

        let mut spiele = self.spiele.clone();
        spiele.sort_by_key(|s| s.teamspiel_nummer);
        let teamspiel_ids = if spiele.len() == units.len() {
            spiele.iter().map(|s| s.teamspiel_id).collect()
        } else {
            Vec::new()
        };

        let labels = Labels {
            tournament: self.veranstaltung.bezeichnung.clone(),
            round: self.begegnung.spieltag.clone(),
            ..Default::default()
        };

        let mut day = TableMatch::new(
            labels,
            team(&self.heim_team, &self.heim_spieler),
            team(&self.gast_team, &self.gast_spieler),
            format,
            units.clone(),
            Competition::Mtfv {
                veranstaltung_id: self.veranstaltung.veranstaltung_id,
                begegnung_id: self.begegnung.begegnung_id,
                teamspiel_ids,
            },
        )?;

        for (unit, spiel) in units.iter().zip(&spiele) {
            for side in [Side::Home, Side::Away] {
                day.prefill_lineup(side, unit.slot(side), spiel_players(spiel, side));
            }
        }
        Ok(day)
    }
}

/// The Begegnungen of a league by Spieltag, in the order the operator needs
/// them: Spieltage still to come first (the next one on top; one played
/// today counts as coming), then the ones already played, the latest first.
/// Within a Spieltag by time, then home team.
pub fn by_spieltag(fixtures: Vec<Fixture>, today: NaiveDate) -> Vec<Vec<Fixture>> {
    let mut groups: Vec<Vec<Fixture>> = Vec::new();
    for fixture in fixtures {
        match groups
            .iter_mut()
            .find(|g| g[0].spieltag == fixture.spieltag)
        {
            Some(group) => group.push(fixture),
            None => groups.push(vec![fixture]),
        }
    }
    for group in &mut groups {
        group.sort_by(|a, b| a.time.cmp(&b.time).then_with(|| a.home.cmp(&b.home)));
    }
    let start = |g: &Vec<Fixture>| g[0].time;
    let last_day = |g: &Vec<Fixture>| g.iter().map(|f| f.time.date()).max();
    let (mut coming, mut played): (Vec<_>, Vec<_>) = groups
        .into_iter()
        .partition(|g| last_day(g).is_some_and(|day| day >= today));
    coming.sort_by_key(start);
    played.sort_by_key(|g| std::cmp::Reverse(start(g)));
    coming.extend(played);
    coming
}

/// A Begegnung as offered to the operator when picking the one on Table 1.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct Fixture {
    pub veranstaltung_id: u32,
    pub league: String,
    pub begegnung_id: u32,
    pub time: NaiveDateTime,
    pub spieltag: String,
    pub home: String,
    pub away: String,
    pub venue: Option<String>,
}

/// Begegnungen on `date` across all leagues, by time, then league order.
/// Selection goes by `zeitpunkt`, not by whether a result exists (ADR 0003).
pub fn fixtures_on(season: &[SeasonLeague], date: NaiveDate) -> Vec<Fixture> {
    let mut fixtures: Vec<Fixture> = season
        .iter()
        .flat_map(|league| {
            league.begegnungen.iter().filter_map(move |b| {
                let time = b.time()?;
                (time.date() == date).then(|| Fixture {
                    veranstaltung_id: league.veranstaltung.veranstaltung_id,
                    league: league.veranstaltung.bezeichnung.clone(),
                    begegnung_id: b.begegnung_id,
                    time,
                    spieltag: b.spieltag.clone(),
                    home: b.heim_name.clone(),
                    away: b.gast_name.clone(),
                    venue: b.spielort_name.clone().filter(|v| !v.is_empty()),
                })
            })
        })
        .collect();
    fixtures.sort_by_key(|f| f.time);
    fixtures
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::competition::{Event, Outcome, Slot, test_support::play_unit};

    const SEASON: &str = include_str!("../../tests/fixtures/mtfv/season.json");
    const VERBANDSLIGA_3132: &str =
        include_str!("../../tests/fixtures/mtfv/begegnung_3132_verbandsliga.json");
    const LANDESLIGA_3080: &str =
        include_str!("../../tests/fixtures/mtfv/begegnung_3080_landesliga.json");
    const LANDESLIGA_3087: &str =
        include_str!("../../tests/fixtures/mtfv/begegnung_3087_landesliga_draw.json");
    const LANDESLIGA_3125: &str =
        include_str!("../../tests/fixtures/mtfv/begegnung_3125_landesliga.json");

    fn detail(json: &str) -> BegegnungDetail {
        client::parse_begegnung(json).unwrap()
    }

    /// Replays the per-unit results stored on mtfv.de through our rules.
    fn replay(d: &BegegnungDetail) -> TableMatch {
        let mut day = d.to_table_match(&FormatRules::default()).unwrap();
        let mut spiele = d.spiele.clone();
        spiele.sort_by_key(|s| s.teamspiel_nummer);
        for s in &spiele {
            let (home, away) = (
                s.teamspiel_heim_punkte.unwrap_or(0),
                s.teamspiel_gast_punkte.unwrap_or(0),
            );
            play_unit(&mut day, home, away);
            // In a set league the operator ends each set and moves on.
            if matches!(day.format, Format::Sets { .. }) {
                let side = if home > away { Side::Home } else { Side::Away };
                let rev = day.revision();
                day.apply(Event::SetWon { side }, rev).unwrap();
                if let Some(unit) = day.next_game() {
                    let rev = day.revision();
                    day.apply(Event::Select { unit }, rev).unwrap();
                }
            }
        }
        day
    }

    #[test]
    fn season_overview_lists_four_leagues() {
        let season = client::parse_season(SEASON).unwrap();
        let names: Vec<_> = season
            .iter()
            .map(|l| l.veranstaltung.bezeichnung.as_str())
            .collect();
        assert_eq!(
            names,
            [
                "Landesliga",
                "Verbandsliga",
                "Bezirksliga Ost",
                "Bezirksliga West"
            ]
        );
        assert_eq!(season[0].tabelle.modus.spielpunkte_bedingung, 6);
    }

    fn league<'a>(season: &'a [SeasonLeague], name: &str) -> &'a SeasonLeague {
        season.iter().find(|l| l.name() == name).unwrap()
    }

    fn fixture(spieltag: &str, time: &str, home: &str) -> Fixture {
        Fixture {
            veranstaltung_id: 72,
            league: "Verbandsliga".into(),
            begegnung_id: 0,
            time: NaiveDateTime::parse_from_str(time, "%Y-%m-%d %H:%M").unwrap(),
            spieltag: spieltag.into(),
            home: home.into(),
            away: "X".into(),
            venue: None,
        }
    }

    #[test]
    fn spieltage_to_come_first_then_the_latest_played() {
        let fixtures = vec![
            fixture("1. Spieltag", "2026-04-19 12:00", "B"),
            fixture("1. Spieltag", "2026-04-19 12:00", "A"),
            fixture("2. Spieltag", "2026-04-19 14:00", "C"),
            fixture("14. Spieltag", "2026-11-07 17:00", "D"),
            fixture("15. Spieltag", "2026-11-15 10:00", "E"),
            fixture("16. Spieltag", "2026-11-21 12:00", "F"),
        ];
        let today = NaiveDate::from_ymd_opt(2026, 11, 15).unwrap();
        let groups = by_spieltag(fixtures, today);
        let order: Vec<Vec<&str>> = groups
            .iter()
            .map(|g| g.iter().map(|f| f.home.as_str()).collect())
            .collect();
        assert_eq!(
            order,
            [vec!["E"], vec!["F"], vec!["D"], vec!["C"], vec!["A", "B"]],
            "today's Spieltag counts as coming; played ones latest first"
        );
    }

    #[test]
    fn leagues_tell_race_from_sets_and_their_games() {
        let season = client::parse_season(SEASON).unwrap();
        let landesliga = league(&season, "Landesliga");
        assert!(landesliga.is_race());
        assert_eq!(landesliga.games().len(), 7);
        let verbandsliga = league(&season, "Verbandsliga");
        assert!(!verbandsliga.is_race());
        let games: Vec<_> = verbandsliga.games();
        assert_eq!(
            games,
            [("D1", 2), ("E1", 2), ("D2", 2), ("E2", 2), ("D3", 2)].map(|(g, n)| (g.to_owned(), n))
        );
        assert_eq!(verbandsliga.fixtures().len(), 56);
    }

    #[test]
    fn a_begegnung_is_built_from_the_overview_alone() {
        let season = client::parse_season(SEASON).unwrap();
        let verbandsliga = league(&season, "Verbandsliga");
        let m = verbandsliga
            .table_match(3132, &FormatRules::default())
            .unwrap();
        assert_eq!(m.units.len(), 10);
        assert!(matches!(m.format, Format::Sets { points: 5, .. }));
        assert_eq!(m.labels.tournament, "Verbandsliga");
        assert!(!m.home.name.is_empty());
        assert!(m.lineups.home.is_empty(), "players are entered live");
        assert!(matches!(
            m.competition,
            Competition::Mtfv {
                begegnung_id: 3132,
                ..
            }
        ));

        let race = league(&season, "Landesliga")
            .table_match(3080, &FormatRules::race_to(42, 6))
            .unwrap();
        assert_eq!(race.format, Format::mtfv_race());
        assert!(matches!(
            verbandsliga.table_match(1, &FormatRules::default()),
            Err(ConvertError::NoSuchBegegnung(1))
        ));
    }

    #[test]
    fn fixtures_of_a_central_match_day() {
        let season = client::parse_season(SEASON).unwrap();
        let date = NaiveDate::from_ymd_opt(2026, 4, 18).unwrap();
        let fixtures = fixtures_on(&season, date);
        let landesliga: Vec<_> = fixtures
            .iter()
            .filter(|f| f.league == "Landesliga")
            .collect();
        assert_eq!(landesliga.len(), 16); // 8 teams, 4 Begegnungen in each of 4 slots
        assert!(fixtures.windows(2).all(|w| w[0].time <= w[1].time));
        assert!(fixtures.iter().any(|f| f.begegnung_id == 3080));
    }

    #[test]
    fn landesliga_is_a_race_with_prefilled_lineups() {
        let d = detail(LANDESLIGA_3080);
        let day = d.to_table_match(&FormatRules::default()).unwrap();
        assert_eq!(day.format, Format::mtfv_race());
        assert_eq!(day.units.len(), 7);
        assert_eq!(day.labels.title(), "Landesliga · 2. Spieltag");
        assert_eq!(
            day.lineups.get(Side::Home, Slot::doubles(1)),
            ["116".to_string(), "495".to_string()]
        );
        assert_eq!(
            day.lineups.get(Side::Away, Slot::singles(1)),
            ["575".to_string()]
        );
        let Competition::Mtfv { teamspiel_ids, .. } = &day.competition else {
            panic!("expected MTFV source");
        };
        assert_eq!(teamspiel_ids.first(), Some(&22076));
    }

    #[test]
    fn verbandsliga_is_two_fixed_sets() {
        let d = detail(VERBANDSLIGA_3132);
        let day = d.to_table_match(&FormatRules::default()).unwrap();
        assert_eq!(day.format, Format::mtfv_sets());
        assert_eq!(day.units.len(), 10);
    }

    #[test]
    fn replayed_results_match_mtfv() {
        for (json, totals, outcome) in [
            (LANDESLIGA_3080, (42, 34), Outcome::Home),
            (LANDESLIGA_3087, (41, 41), Outcome::Draw),
            (LANDESLIGA_3125, (30, 42), Outcome::Away),
        ] {
            let d = detail(json);
            let day = replay(&d);
            let p = day.progress();
            assert_eq!(p.totals(), totals, "Begegnung {}", d.begegnung.begegnung_id);
            assert_eq!(p.outcome, Some(outcome));
            let mut spiele = d.spiele.clone();
            spiele.sort_by_key(|s| s.teamspiel_nummer);
            let expected: Vec<_> = spiele
                .iter()
                .map(|s| {
                    (
                        s.teamspiel_heim_punkte.unwrap_or(0),
                        s.teamspiel_gast_punkte.unwrap_or(0),
                    )
                })
                .collect();
            assert_eq!(p.scores, expected);
        }

        let d = detail(VERBANDSLIGA_3132);
        let day = replay(&d);
        let p = day.progress();
        assert_eq!(day.match_points(&p), (9, 1));
        assert_eq!(p.outcome, Some(Outcome::Home));
    }
}
