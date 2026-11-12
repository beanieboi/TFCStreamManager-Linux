//! Match day templates as written in the MTFV `modus` string (ADR 0004).
//!
//! A `modus` is a comma-separated list of tokens, one per unit (set or
//! segment). A token is a home slot, an away slot and an optional `H`/`G`
//! flag: `D1D1H` is home D1 against away D1, flagged home.

use serde::{Deserialize, Serialize};
use std::fmt;
use std::str::FromStr;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Side {
    Home,
    Away,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SlotKind {
    Doubles,
    Singles,
}

impl SlotKind {
    pub fn players(self) -> usize {
        match self {
            SlotKind::Doubles => 2,
            SlotKind::Singles => 1,
        }
    }
}

/// A lineup position on the Spielbericht, e.g. `D1` or `E2`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct Slot {
    pub kind: SlotKind,
    pub number: u8,
}

impl Slot {
    pub fn doubles(number: u8) -> Self {
        Self {
            kind: SlotKind::Doubles,
            number,
        }
    }

    pub fn singles(number: u8) -> Self {
        Self {
            kind: SlotKind::Singles,
            number,
        }
    }
}

impl fmt::Display for Slot {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let prefix = match self.kind {
            SlotKind::Doubles => 'D',
            SlotKind::Singles => 'E',
        };
        write!(f, "{prefix}{}", self.number)
    }
}

impl FromStr for Slot {
    type Err = ModusError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match parse_slot(s) {
            Some((slot, "")) => Ok(slot),
            _ => Err(ModusError::InvalidSlot(s.to_owned())),
        }
    }
}

impl TryFrom<String> for Slot {
    type Error = ModusError;

    fn try_from(s: String) -> Result<Self, Self::Error> {
        s.parse()
    }
}

impl From<Slot> for String {
    fn from(slot: Slot) -> Self {
        slot.to_string()
    }
}

/// One unit of a match day template.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct UnitSpec {
    pub home: Slot,
    pub away: Slot,
    /// `H`/`G` from the token. Stored, not used (ADR 0004).
    pub flag: Option<Side>,
}

impl UnitSpec {
    pub fn slot(&self, side: Side) -> Slot {
        match side {
            Side::Home => self.home,
            Side::Away => self.away,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ModusError {
    #[error("modus is empty")]
    Empty,
    #[error("invalid modus token {0:?}")]
    InvalidToken(String),
    #[error("invalid slot {0:?}")]
    InvalidSlot(String),
}

fn parse_slot(s: &str) -> Option<(Slot, &str)> {
    let kind = match s.chars().next()? {
        'D' => SlotKind::Doubles,
        'E' => SlotKind::Singles,
        _ => return None,
    };
    let rest = &s[1..];
    let end = rest
        .find(|c: char| !c.is_ascii_digit())
        .unwrap_or(rest.len());
    if end == 0 {
        return None;
    }
    let number = rest[..end].parse().ok()?;
    Some((Slot { kind, number }, &rest[end..]))
}

fn parse_token(token: &str) -> Option<UnitSpec> {
    let (home, rest) = parse_slot(token)?;
    let (away, rest) = parse_slot(rest)?;
    let flag = match rest {
        "" => None,
        "H" => Some(Side::Home),
        "G" => Some(Side::Away),
        _ => return None,
    };
    Some(UnitSpec { home, away, flag })
}

pub fn parse_modus(modus: &str) -> Result<Vec<UnitSpec>, ModusError> {
    let units = modus
        .split(',')
        .map(str::trim)
        .filter(|t| !t.is_empty())
        .map(|t| parse_token(t).ok_or_else(|| ModusError::InvalidToken(t.to_owned())))
        .collect::<Result<Vec<_>, _>>()?;
    if units.is_empty() {
        return Err(ModusError::Empty);
    }
    Ok(units)
}

/// Lengths of consecutive runs of units with the same slot pair.
/// In a set-based format each run is one game.
pub fn game_groups(units: &[UnitSpec]) -> Vec<std::ops::Range<usize>> {
    let mut groups = Vec::new();
    let mut start = 0;
    for i in 1..=units.len() {
        let boundary = i == units.len()
            || (units[i].home, units[i].away) != (units[start].home, units[start].away);
        if boundary {
            groups.push(start..i);
            start = i;
        }
    }
    groups
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_verbandsliga() {
        let units =
            parse_modus("D1D1H,D1D1G,E1E1H,E1E1G,D2D2H,D2D2G,E2E2H,E2E2G,D3D3H,D3D3G").unwrap();
        assert_eq!(units.len(), 10);
        assert_eq!(units[0].home, Slot::doubles(1));
        assert_eq!(units[0].flag, Some(Side::Home));
        assert_eq!(units[1].flag, Some(Side::Away));
        assert_eq!(units[2].home, Slot::singles(1));
        assert_eq!(game_groups(&units).len(), 5);
        assert!(game_groups(&units).iter().all(|g| g.len() == 2));
    }

    #[test]
    fn parses_landesliga_race() {
        let units = parse_modus("D1D1,D2D2,E1E1,D3D3,E2E2,D4D4,D5D5").unwrap();
        assert_eq!(units.len(), 7);
        assert!(units.iter().all(|u| u.flag.is_none()));
        assert_eq!(units[2].away, Slot::singles(1));
    }

    #[test]
    fn parses_bezirksliga_with_single_flag() {
        let units =
            parse_modus("D1D1H,D1D1,D2D2,D2D2,E1E1,E1E1,D3D3,D3D3,E2E2,E2E2,D4D4,D4D4,D5D5,D5D5")
                .unwrap();
        assert_eq!(units.len(), 14);
        assert_eq!(game_groups(&units).len(), 7);
    }

    #[test]
    fn parses_crossover_and_multi_digit() {
        let units = parse_modus("D1D2,E10E3G").unwrap();
        assert_eq!(units[0].away, Slot::doubles(2));
        assert_eq!(units[1].home, Slot::singles(10));
        assert_eq!(units[1].away, Slot::singles(3));
        assert_eq!(units[1].flag, Some(Side::Away));
    }

    #[test]
    fn rejects_garbage() {
        assert_eq!(parse_modus(""), Err(ModusError::Empty));
        assert!(matches!(
            parse_modus("D1"),
            Err(ModusError::InvalidToken(_))
        ));
        assert!(matches!(
            parse_modus("D1D1X"),
            Err(ModusError::InvalidToken(_))
        ));
        assert!(matches!(
            parse_modus("X1D1"),
            Err(ModusError::InvalidToken(_))
        ));
    }

    #[test]
    fn slot_roundtrips_as_string() {
        let slot: Slot = serde_json::from_str("\"E2\"").unwrap();
        assert_eq!(slot, Slot::singles(2));
        assert_eq!(serde_json::to_string(&slot).unwrap(), "\"E2\"");
        assert!("D".parse::<Slot>().is_err());
    }
}
