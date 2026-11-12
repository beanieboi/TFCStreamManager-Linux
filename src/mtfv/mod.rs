//! MTFV league source (ADR 0003): season overview, Begegnung details, and
//! conversion into a source-independent [`MatchDay`].

// Wired into the web server and UI in the next step.
#![allow(dead_code, unused_imports)]

mod client;
mod models;

pub use client::MtfvClient;
pub use models::{BegegnungDetail, BegegnungSummary, Modus, SeasonLeague};

use chrono::{NaiveDate, NaiveDateTime};

use crate::competition::{
    CommandError, Format, MatchDay, ModusError, Player, SetCount, Side, Source, Team, UnitSpec,
    game_groups, parse_modus,
};
use models::{Spiel, Spieler, TeamInfo};

/// Format parameters that are not in the mtfv.de data (ADR 0005).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FormatRules {
    pub race_target: u16,
    pub race_draw_at: Option<u16>,
    pub set_win_by: u16,
    pub set_cap: Option<u16>,
}

impl Default for FormatRules {
    fn default() -> Self {
        Self {
            race_target: 42,
            race_draw_at: Some(41),
            set_win_by: 2,
            set_cap: None,
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
            segment: modus.spielpunkte_bedingung as u16,
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
    MatchDay(#[from] CommandError),
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
    pub fn to_match_day(&self, rules: &FormatRules) -> Result<MatchDay, ConvertError> {
        let units = parse_modus(&self.modus.modus)?;
        let format = format_for(&self.modus, &units, rules);

        let mut spiele = self.spiele.clone();
        spiele.sort_by_key(|s| s.teamspiel_nummer);
        let teamspiel_ids = if spiele.len() == units.len() {
            spiele.iter().map(|s| s.teamspiel_id).collect()
        } else {
            Vec::new()
        };

        let title = [
            self.veranstaltung.bezeichnung.as_str(),
            self.begegnung.spieltag.as_str(),
        ]
        .into_iter()
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join(" · ");

        let mut day = MatchDay::new(
            title,
            team(&self.heim_team, &self.heim_spieler),
            team(&self.gast_team, &self.gast_spieler),
            format,
            units.clone(),
            Source::Mtfv {
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
    use crate::competition::{Outcome, Slot, test_support::play_unit};

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
    fn replay(d: &BegegnungDetail) -> MatchDay {
        let mut day = d.to_match_day(&FormatRules::default()).unwrap();
        let mut spiele = d.spiele.clone();
        spiele.sort_by_key(|s| s.teamspiel_nummer);
        for s in &spiele {
            play_unit(
                &mut day,
                s.teamspiel_heim_punkte.unwrap_or(0),
                s.teamspiel_gast_punkte.unwrap_or(0),
            );
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
        let day = d.to_match_day(&FormatRules::default()).unwrap();
        assert_eq!(day.format, Format::mtfv_race());
        assert_eq!(day.units.len(), 7);
        assert_eq!(day.title, "Landesliga · 2. Spieltag");
        assert_eq!(
            day.lineups.get(Side::Home, Slot::doubles(1)),
            ["116".to_string(), "495".to_string()]
        );
        assert_eq!(
            day.lineups.get(Side::Away, Slot::singles(1)),
            ["575".to_string()]
        );
        let Source::Mtfv { teamspiel_ids, .. } = &day.source else {
            panic!("expected MTFV source");
        };
        assert_eq!(teamspiel_ids.first(), Some(&22076));
    }

    #[test]
    fn verbandsliga_is_two_fixed_sets() {
        let d = detail(VERBANDSLIGA_3132);
        let day = d.to_match_day(&FormatRules::default()).unwrap();
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
