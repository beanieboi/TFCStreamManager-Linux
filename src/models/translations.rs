//! Overlay names for Kickertool rounds, groups and disciplines, from
//! `translations.toml` (bundled, optionally overridden in the config dir).

use serde::Deserialize;
use std::collections::BTreeMap;
use std::path::Path;

use crate::competition::{Slot, SlotKind};

const BUNDLED: &str = include_str!("../../translations.toml");
const NUMBER: &str = "{n}";

#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Translations {
    rounds: BTreeMap<String, String>,
    groups: BTreeMap<String, String>,
    disciplines: DisciplineNames,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct DisciplineNames {
    doubles: Option<String>,
    singles: Option<String>,
}

impl Translations {
    pub fn bundled() -> Self {
        toml::from_str(BUNDLED).expect("bundled translations.toml is valid")
    }

    /// The bundled translations with the entries of `path` on top. A missing
    /// file is not an error.
    pub fn load(path: &Path) -> anyhow::Result<Self> {
        let mut translations = Self::bundled();
        match std::fs::read_to_string(path) {
            Ok(content) => translations.merge(toml::from_str(&content)?),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(e.into()),
        }
        Ok(translations)
    }

    fn merge(&mut self, other: Self) {
        self.rounds.extend(other.rounds);
        self.groups.extend(other.groups);
        if other.disciplines.doubles.is_some() {
            self.disciplines.doubles = other.disciplines.doubles;
        }
        if other.disciplines.singles.is_some() {
            self.disciplines.singles = other.disciplines.singles;
        }
    }

    pub fn round(&self, name: &str) -> String {
        lookup(&self.rounds, name)
    }

    pub fn group(&self, name: &str) -> String {
        lookup(&self.groups, name)
    }

    /// The game of an MTFV Begegnung, e.g. "Doppel 1" for `D1`: the
    /// discipline names with the number; the slot itself without one.
    pub fn game(&self, slot: Slot) -> String {
        let name = match slot.kind {
            SlotKind::Doubles => self.disciplines.doubles.as_deref(),
            SlotKind::Singles => self.disciplines.singles.as_deref(),
        };
        name.map_or_else(
            || slot.to_string(),
            |name| format!("{name} {}", slot.number),
        )
    }

    /// Name for an unnamed discipline, by the most players in one team.
    pub fn unnamed_discipline(&self, players: usize) -> Option<&str> {
        match players {
            0 => None,
            1 => self.disciplines.singles.as_deref(),
            _ => self.disciplines.doubles.as_deref(),
        }
    }
}

/// Exact key first, then keys with `{n}` standing for a number.
fn lookup(map: &BTreeMap<String, String>, name: &str) -> String {
    if let Some(text) = map.get(name) {
        return text.clone();
    }
    map.iter()
        .find_map(|(key, text)| {
            let (prefix, suffix) = key.split_once(NUMBER)?;
            let n = name.strip_prefix(prefix)?.strip_suffix(suffix)?;
            (!n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()))
                .then(|| text.replace(NUMBER, n))
        })
        .unwrap_or_else(|| name.to_owned())
}

#[cfg(test)]
impl Translations {
    /// Fixed translations for tests, independent of the editable bundled file.
    pub fn fixture() -> Self {
        toml::from_str(
            r#"
            [rounds]
            "FINALS-1-2" = "Semi"
            "FINALS-1-{n}" = "1/{n} final"
            "Round {n}" = "R{n}"
            [groups]
            "Qualification" = "Quali"
            [disciplines]
            doubles = "Doubles"
            singles = "Singles"
            "#,
        )
        .unwrap()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bundled_file_is_valid() {
        Translations::bundled();
    }

    #[test]
    fn exact_key_wins_over_pattern() {
        let t = Translations::fixture();
        assert_eq!(t.round("FINALS-1-2"), "Semi");
        assert_eq!(t.round("FINALS-1-4"), "1/4 final");
    }

    #[test]
    fn pattern_needs_a_number() {
        let t = Translations::fixture();
        assert_eq!(t.round("Round 12"), "R12");
        assert_eq!(t.round("Round "), "Round ");
        assert_eq!(t.round("Round x"), "Round x");
        assert_eq!(t.round("FINALS-1-x"), "FINALS-1-x");
    }

    #[test]
    fn unknown_names_are_kept() {
        let t = Translations::fixture();
        assert_eq!(t.round("BASE_ROUND"), "BASE_ROUND");
        assert_eq!(t.group("Gruppe A"), "Gruppe A");
        assert_eq!(t.round(""), "");
    }

    #[test]
    fn groups_and_disciplines() {
        let t = Translations::fixture();
        assert_eq!(t.group("Qualification"), "Quali");
        assert_eq!(t.unnamed_discipline(2), Some("Doubles"));
        assert_eq!(t.unnamed_discipline(3), Some("Doubles"));
        assert_eq!(t.unnamed_discipline(1), Some("Singles"));
        assert_eq!(t.unnamed_discipline(0), None);
    }

    #[test]
    fn games_are_named_by_discipline() {
        let t = Translations::fixture();
        assert_eq!(t.game(Slot::doubles(2)), "Doubles 2");
        assert_eq!(t.game(Slot::singles(1)), "Singles 1");
        assert_eq!(Translations::default().game(Slot::singles(1)), "E1");
    }

    #[test]
    fn override_replaces_single_entries() {
        let mut t = Translations::fixture();
        t.merge(
            toml::from_str(
                r#"
                [rounds]
                "Round {n}" = "Runde {n}"
                [disciplines]
                doubles = "DYP"
                "#,
            )
            .unwrap(),
        );
        assert_eq!(t.round("Round 1"), "Runde 1");
        assert_eq!(t.round("FINALS-1-2"), "Semi");
        assert_eq!(t.unnamed_discipline(2), Some("DYP"));
        assert_eq!(t.unnamed_discipline(1), Some("Singles"));
    }

    #[test]
    fn load_missing_file_is_bundled() {
        let t = Translations::load(Path::new("/nonexistent/translations.toml")).unwrap();
        assert_eq!(t, Translations::bundled());
    }

    #[test]
    fn unknown_section_is_an_error() {
        assert!(toml::from_str::<Translations>("[round]\n\"a\" = \"b\"").is_err());
    }
}
