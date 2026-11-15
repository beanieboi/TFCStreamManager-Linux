//! Overlay templates (ADR 0010). They all come from one folder, the overlay
//! folder; none ship with the app, so none go stale. The folder is read on
//! every request, and [`OverlayLibrary::fingerprint`] lets open pages reload
//! when a file changes.
//!
//! `overlay.js` is not a template but the protocol between server and page;
//! it ships with the app, read from the source tree when that is there.

use std::borrow::Cow;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};

const EXTENSION: &str = "html";

/// Binds a template's `{{placeholders}}` to `/api/overlay` (ADR 0011).
const SCRIPT: &str = include_str!("../../assets/overlay.js");
const SCRIPT_FILE: &str = "overlay.js";

/// `linux/assets/` in the source tree, when the app runs from it, so the
/// script can be edited without a rebuild.
fn source_dir() -> Option<PathBuf> {
    let dir = PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/assets"));
    dir.is_dir().then_some(dir)
}

/// The format a template is made for (`tfc:format`). It decides the format
/// of a Kickertool or Frei match while the template is on air.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TemplateFormat {
    Sets,
    Race,
}

/// Who a template is made for (`tfc:players`): one player per side, or two.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TemplatePlayers {
    Singles,
    Doubles,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Template {
    /// File name without `.html`; the template is served at `/overlay/<id>`.
    pub id: String,
    /// `tfc:name`, or the id.
    pub name: String,
    /// `tfc:sources`; `None` fits every source.
    sources: Option<Vec<String>>,
    /// `tfc:format`; `None` leaves the format alone.
    pub format: Option<TemplateFormat>,
    /// `tfc:players`; `None` fits singles and doubles.
    pub players: Option<TemplatePlayers>,
    pub html: String,
}

impl Template {
    fn new(id: String, html: String) -> Self {
        let name = meta(&html, "tfc:name")
            .filter(|n| !n.trim().is_empty())
            .unwrap_or_else(|| id.clone());
        let sources =
            meta(&html, "tfc:sources").map(|s| s.split_whitespace().map(str::to_owned).collect());
        let format = match meta(&html, "tfc:format").as_deref().map(str::trim) {
            Some("race") => Some(TemplateFormat::Race),
            Some("sets") => Some(TemplateFormat::Sets),
            _ => None,
        };
        let players = match meta(&html, "tfc:players").as_deref().map(str::trim) {
            Some("singles") => Some(TemplatePlayers::Singles),
            Some("doubles") => Some(TemplatePlayers::Doubles),
            _ => None,
        };
        Self {
            id,
            name,
            sources,
            format,
            players,
            html,
        }
    }

    /// Whether the server's `{{placeholder}}` appears in the template.
    pub fn uses(&self, placeholder: &str) -> bool {
        self.html.contains(&format!("{{{{{placeholder}}}}}"))
    }

    /// Whether the template fits a selection key: a source (`kickertool`),
    /// or a source with what the match on the table demands: its format
    /// (`mtfv:sets`) or its players (`kickertool:doubles`).
    pub fn fits(&self, key: &str) -> bool {
        let source = key.split_once(':').map_or(key, |(source, _)| source);
        let source_fits = self
            .sources
            .as_ref()
            .is_none_or(|sources| sources.iter().any(|s| s == source));
        source_fits && self.fits_match(key)
    }

    /// Whether the template can show the match a key describes: a race
    /// template never serves a set league, a singles template never a
    /// doubles match, and a Frei Einzel or Doppel is played in sets. The
    /// source does not matter here; the operator may choose any template for
    /// a competition.
    pub fn fits_match(&self, key: &str) -> bool {
        let (format, players) = demands(key);
        format.is_none_or(|f| self.format.is_none_or(|own| own == f))
            && players.is_none_or(|p| self.players.is_none_or(|own| own == p))
    }

    /// How many of the key's demands the template declares itself (rather
    /// than not contradicting them). The automatic choice prefers more.
    fn declares(&self, key: &str) -> usize {
        let (format, players) = demands(key);
        usize::from(format.is_some() && self.format == format)
            + usize::from(players.is_some() && self.players == players)
    }
}

/// Clones share the folder, so choosing another one in the settings applies
/// to the server and the main window at once.
#[derive(Debug, Clone)]
pub struct OverlayLibrary {
    dir: Arc<RwLock<PathBuf>>,
    /// Where `overlay.js` is read from, if not the copy compiled in.
    script_dir: Option<PathBuf>,
}

impl OverlayLibrary {
    pub fn new(dir: PathBuf) -> Self {
        Self {
            dir: Arc::new(RwLock::new(dir)),
            script_dir: source_dir(),
        }
    }

    pub fn dir(&self) -> PathBuf {
        self.dir.read().expect("overlay dir lock").clone()
    }

    pub fn set_dir(&self, dir: PathBuf) {
        *self.dir.write().expect("overlay dir lock") = dir;
    }

    /// The script the server adds to every overlay page.
    pub fn script(&self) -> Cow<'static, str> {
        self.script_dir
            .as_ref()
            .and_then(|dir| std::fs::read_to_string(dir.join(SCRIPT_FILE)).ok())
            .map_or(Cow::Borrowed(SCRIPT), Cow::Owned)
    }

    /// Changes when a template or the script is added, removed or edited.
    pub fn fingerprint(&self) -> u64 {
        use std::hash::{Hash, Hasher};
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        let script = self.script_dir.as_ref().map(|d| d.join(SCRIPT_FILE));
        let mut files: Vec<_> = std::fs::read_dir(self.dir())
            .into_iter()
            .flatten()
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .filter(|path| path.extension().is_some_and(|e| e == EXTENSION))
            .chain(script)
            .filter_map(|path| {
                let meta = std::fs::metadata(&path).ok()?;
                Some((path, meta.modified().ok(), meta.len()))
            })
            .collect();
        files.sort();
        files.hash(&mut hasher);
        hasher.finish()
    }

    /// The templates in the folder, by file name.
    pub fn templates(&self) -> Vec<Template> {
        self.read_files()
            .into_iter()
            .map(|(id, html)| Template::new(id, html))
            .collect()
    }

    pub fn get(&self, id: &str) -> Option<Template> {
        self.templates().into_iter().find(|t| t.id == id)
    }

    fn read_files(&self) -> BTreeMap<String, String> {
        let Ok(entries) = std::fs::read_dir(self.dir()) else {
            return BTreeMap::new();
        };
        entries
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .filter(|path| path.extension().is_some_and(|e| e == EXTENSION))
            .filter_map(|path| {
                let id = path.file_stem()?.to_str()?.to_owned();
                if id.starts_with('.') {
                    return None;
                }
                match std::fs::read_to_string(&path) {
                    Ok(html) => Some((id, html)),
                    Err(e) => {
                        tracing::warn!("Skipping overlay {}: {e}", path.display());
                        None
                    }
                }
            })
            .collect()
    }

    /// Copies (or moves) an overlay that was configured by path before
    /// ADR 0010 into the overlay folder. Returns where it went, or `None`
    /// when a file of that name already exists.
    pub fn adopt(&self, file: &Path, move_file: bool) -> std::io::Result<Option<PathBuf>> {
        let html = std::fs::read_to_string(file)?;
        let Some(name) = file.file_name() else {
            return Ok(None);
        };
        let dir = self.dir();
        let target = dir.join(name).with_extension(EXTENSION);
        if target.exists() {
            return Ok(None);
        }
        std::fs::create_dir_all(&dir)?;
        if move_file {
            std::fs::rename(file, &target)?;
        } else {
            std::fs::write(&target, html)?;
        }
        Ok(Some(target))
    }
}

/// What a selection key demands of a template: a format and/or players.
/// `manual:singles` and `manual:doubles` are played in sets.
fn demands(key: &str) -> (Option<TemplateFormat>, Option<TemplatePlayers>) {
    let Some((source, demand)) = key.split_once(':') else {
        return (None, None);
    };
    let frei_sets = (source == "manual").then_some(TemplateFormat::Sets);
    match demand {
        "race" => (Some(TemplateFormat::Race), None),
        "sets" => (Some(TemplateFormat::Sets), None),
        "singles" => (frei_sets, Some(TemplatePlayers::Singles)),
        "doubles" => (frei_sets, Some(TemplatePlayers::Doubles)),
        _ => (None, None),
    }
}

/// The template for a selection key (see [`Template::fits`]): the
/// operator's choice for that key while it still exists and fits (a race
/// template never serves a set league), else the first one fitting it, else
/// the first one.
pub fn select<'a>(
    templates: &'a [Template],
    key: Option<&str>,
    choices: &BTreeMap<String, String>,
) -> Option<&'a Template> {
    let Some(key) = key else {
        return templates.first();
    };
    // The first of the best: templates are ordered by file name.
    let automatic = || {
        let fitting = templates.iter().filter(|t| t.fits(key));
        let best = fitting.clone().map(|t| t.declares(key)).max()?;
        fitting.clone().find(|t| t.declares(key) == best)
    };
    choices
        .get(key)
        .and_then(|id| templates.iter().find(|t| &t.id == id))
        .filter(|t| t.fits_match(key))
        .or_else(automatic)
        .or_else(|| templates.first())
}

/// `content` of `<meta name="{name}">` in the head.
fn meta(html: &str, name: &str) -> Option<String> {
    let head = html.split("</head>").next().unwrap_or(html);
    let mut rest = head;
    while let Some(start) = rest.find("<meta") {
        let after = &rest[start + "<meta".len()..];
        let end = after.find('>')?;
        let tag = &after[..end];
        if attribute(tag, "name").as_deref() == Some(name) {
            return attribute(tag, "content");
        }
        rest = &after[end..];
    }
    None
}

fn attribute(tag: &str, attr: &str) -> Option<String> {
    let mut rest = tag;
    loop {
        let i = rest.find(attr)?;
        let starts_word = i == 0 || rest.as_bytes()[i - 1].is_ascii_whitespace();
        let after = &rest[i + attr.len()..];
        if starts_word && let Some(value) = after.trim_start().strip_prefix('=') {
            let value = value.trim_start();
            return match value.chars().next()? {
                quote @ ('"' | '\'') => {
                    let inner = &value[1..];
                    Some(inner[..inner.find(quote)?].to_owned())
                }
                _ => {
                    let end = value
                        .find(|c: char| c.is_ascii_whitespace() || c == '/')
                        .unwrap_or(value.len());
                    Some(value[..end].to_owned())
                }
            };
        }
        rest = after;
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn template(id: &str, head: &str) -> Template {
        Template::new(
            id.to_owned(),
            format!("<html><head>{head}</head><body></body></html>"),
        )
    }

    pub(crate) fn temp_dir(name: &str) -> PathBuf {
        static COUNTER: AtomicUsize = AtomicUsize::new(0);
        let dir = std::env::temp_dir().join(format!(
            "tfc-overlays-{name}-{}-{}",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// A folder with two templates, as a test fixture: one for Kickertool
    /// and Frei in sets, one for MTFV races.
    pub(crate) fn test_library() -> OverlayLibrary {
        let library = OverlayLibrary::new(temp_dir("fixture"));
        let dir = library.dir();
        std::fs::write(
            dir.join("player_overlay.html"),
            r#"<html><head><meta name="tfc:name" content="Turnier Einzel">
<meta name="tfc:sources" content="kickertool manual">
<meta name="tfc:format" content="sets"></head>
<body><div>{{teamA}} {{scoreA}}</div></body></html>"#,
        )
        .unwrap();
        std::fs::write(
            dir.join("mtfv_race_42.html"),
            r#"<html><head><meta name="tfc:name" content="MTFV Race 42">
<meta name="tfc:sources" content="mtfv">
<meta name="tfc:format" content="race"></head>
<body><div>{{teamA}} {{segmentNumber}}</div></body></html>"#,
        )
        .unwrap();
        library
    }

    #[test]
    fn meta_tags_in_any_form() {
        let t = template(
            "a",
            r#"<meta charset="utf-8"><meta content='Team' name="tfc:name" /><meta name=tfc:sources content="mtfv manual">"#,
        );
        assert_eq!(t.name, "Team");
        assert!(t.fits("mtfv"));
        assert!(t.fits("manual"));
        assert!(!t.fits("kickertool"));
    }

    #[test]
    fn without_meta_tags_named_by_id_and_fits_all() {
        let t = template("my_overlay", "");
        assert_eq!(t.name, "my_overlay");
        assert!(t.fits("kickertool"));
        assert_eq!(t.format, None);
    }

    #[test]
    fn placeholders_are_matched_whole() {
        let t = Template::new("a".into(), "{{teamAPlayer}} {{scoreA}}".into());
        assert!(t.uses("teamAPlayer"));
        assert!(t.uses("scoreA"));
        assert!(!t.uses("teamA"));
        assert!(!t.uses("score"));
    }

    #[test]
    fn meta_outside_head_is_ignored() {
        let t = Template::new(
            "a".into(),
            r#"<head></head><body><meta name="tfc:name" content="X"></body>"#.into(),
        );
        assert_eq!(t.name, "a");
    }

    #[test]
    fn clones_share_the_folder() {
        let library = test_library();
        let clone = library.clone();
        clone.set_dir(temp_dir("other"));
        assert!(library.templates().is_empty());
    }

    #[test]
    fn nothing_is_built_in() {
        let library = OverlayLibrary::new(temp_dir("empty"));
        assert!(library.templates().is_empty());
        assert!(library.get("player_overlay").is_none());
    }

    #[test]
    fn templates_declare_sources_and_formats() {
        let library = test_library();
        let templates = library.templates();
        let pick = |source| select(&templates, Some(source), &BTreeMap::new()).unwrap();
        assert_eq!(pick("kickertool").id, "player_overlay");
        assert_eq!(pick("mtfv").id, "mtfv_race_42");
        let format = |id| library.get(id).unwrap().format;
        assert_eq!(format("player_overlay"), Some(TemplateFormat::Sets));
        assert_eq!(format("mtfv_race_42"), Some(TemplateFormat::Race));
    }

    #[test]
    fn edits_are_read_on_every_call() {
        let library = test_library();
        std::fs::write(
            library.dir().join("player_overlay.html"),
            "<head></head>edited",
        )
        .unwrap();
        assert!(
            library
                .get("player_overlay")
                .unwrap()
                .html
                .ends_with("edited")
        );
        std::fs::write(library.dir().join("zeta.html"), "<head></head>").unwrap();
        assert!(library.get("zeta").is_some());
        std::fs::write(library.dir().join(".hidden.html"), "").unwrap();
        std::fs::write(library.dir().join("notes.txt"), "").unwrap();
        assert_eq!(library.templates().len(), 3);
    }

    #[test]
    fn fingerprint_follows_file_changes() {
        let library = OverlayLibrary {
            script_dir: None,
            ..OverlayLibrary::new(temp_dir("fingerprint"))
        };
        let dir = library.dir();
        let empty = library.fingerprint();
        std::fs::write(dir.join("a.html"), "<head></head>1").unwrap();
        let one = library.fingerprint();
        assert_ne!(empty, one);
        assert_eq!(one, library.fingerprint(), "stable without changes");
        std::fs::write(dir.join("a.html"), "<head></head>longer").unwrap();
        assert_ne!(one, library.fingerprint(), "an edit changes it");
        std::fs::write(dir.join("notes.txt"), "x").unwrap();
        let before = library.fingerprint();
        std::fs::write(dir.join("notes.txt"), "xy").unwrap();
        assert_eq!(before, library.fingerprint(), "other files do not count");
    }

    #[test]
    fn script_from_the_source_tree_or_compiled_in() {
        let dir = temp_dir("script");
        std::fs::write(dir.join("overlay.js"), "// edited").unwrap();
        let from_source = OverlayLibrary {
            script_dir: Some(dir),
            ..OverlayLibrary::new(PathBuf::from("/nonexistent"))
        };
        assert_eq!(from_source.script(), "// edited");
        let compiled = OverlayLibrary {
            script_dir: None,
            ..OverlayLibrary::new(PathBuf::from("/nonexistent"))
        };
        assert_eq!(compiled.script(), SCRIPT);
    }

    #[test]
    fn mtfv_templates_are_picked_by_format() {
        let templates = [
            template(
                "race",
                r#"<meta name="tfc:sources" content="mtfv"><meta name="tfc:format" content="race">"#,
            ),
            template(
                "sets",
                r#"<meta name="tfc:sources" content="mtfv"><meta name="tfc:format" content="sets">"#,
            ),
        ];
        let mut choices = BTreeMap::new();
        assert_eq!(
            select(&templates, Some("mtfv:race"), &choices).unwrap().id,
            "race"
        );
        assert_eq!(
            select(&templates, Some("mtfv:sets"), &choices).unwrap().id,
            "sets"
        );
        choices.insert("mtfv:sets".to_owned(), "race".to_owned());
        assert_eq!(
            select(&templates, Some("mtfv:sets"), &choices).unwrap().id,
            "sets",
            "a choice that does not fit the format is ignored"
        );
        choices.insert("mtfv:race".to_owned(), "race".to_owned());
        assert_eq!(
            select(&templates, Some("mtfv:race"), &choices).unwrap().id,
            "race"
        );
        assert_eq!(
            select(&templates, Some("mtfv:race"), &choices).unwrap().id,
            "race"
        );
        assert!(
            template("any", "").fits("mtfv:sets"),
            "no tfc:format fits all"
        );
    }

    #[test]
    fn kickertool_templates_are_picked_by_players() {
        let templates = [
            template(
                "einzel",
                r#"<meta name="tfc:sources" content="kickertool"><meta name="tfc:players" content="singles">"#,
            ),
            template(
                "doppel",
                r#"<meta name="tfc:sources" content="kickertool"><meta name="tfc:players" content="doubles">"#,
            ),
            template("race", r#"<meta name="tfc:format" content="race">"#),
        ];
        let mut choices = BTreeMap::new();
        let pick = |key, choices: &BTreeMap<String, String>| {
            select(&templates, Some(key), choices).unwrap().id.clone()
        };
        assert_eq!(pick("kickertool:singles", &choices), "einzel");
        assert_eq!(pick("kickertool:doubles", &choices), "doppel");
        choices.insert("kickertool:doubles".into(), "einzel".into());
        assert_eq!(
            pick("kickertool:doubles", &choices),
            "doppel",
            "singles never for doubles"
        );
        choices.insert("kickertool:doubles".into(), "race".into());
        assert_eq!(
            pick("kickertool:doubles", &choices),
            "race",
            "no tfc:players fits both"
        );
    }

    #[test]
    fn frei_kinds_pick_their_templates() {
        let templates = [
            template(
                "a_race",
                r#"<meta name="tfc:sources" content="mtfv manual"><meta name="tfc:format" content="race">"#,
            ),
            template(
                "einzel",
                r#"<meta name="tfc:sources" content="kickertool manual"><meta name="tfc:format" content="sets"><meta name="tfc:players" content="singles">"#,
            ),
            template(
                "doppel",
                r#"<meta name="tfc:sources" content="kickertool manual"><meta name="tfc:format" content="sets"><meta name="tfc:players" content="doubles">"#,
            ),
            template("plain", ""),
        ];
        let pick = |key| {
            select(&templates, Some(key), &BTreeMap::new())
                .unwrap()
                .id
                .clone()
        };
        assert_eq!(
            pick("manual:singles"),
            "einzel",
            "not the race template that sorts first"
        );
        assert_eq!(pick("manual:doubles"), "doppel");
        assert_eq!(pick("manual:race"), "a_race");
        assert!(
            !templates[0].fits_match("manual:doubles"),
            "a Frei Doppel is sets"
        );
        assert!(
            templates[3].fits_match("manual:doubles"),
            "a plain template fits all"
        );
    }

    #[test]
    fn selection_prefers_the_choice_while_it_exists() {
        let templates = [
            template("a", r#"<meta name="tfc:sources" content="manual">"#),
            template("b", r#"<meta name="tfc:sources" content="kickertool">"#),
        ];
        let mut choices = BTreeMap::new();
        assert_eq!(
            select(&templates, Some("kickertool"), &choices).unwrap().id,
            "b"
        );
        choices.insert("kickertool".to_owned(), "a".to_owned());
        assert_eq!(
            select(&templates, Some("kickertool"), &choices).unwrap().id,
            "a"
        );
        choices.insert("kickertool".to_owned(), "gone".to_owned());
        assert_eq!(
            select(&templates, Some("kickertool"), &choices).unwrap().id,
            "b"
        );
        assert_eq!(select(&templates, Some("other"), &choices).unwrap().id, "a");
        assert_eq!(select(&templates, None, &choices).unwrap().id, "a");
        assert!(select(&[], Some("kickertool"), &choices).is_none());
    }

    #[test]
    fn adopt_copies_custom_files_once() {
        let dir = temp_dir("adopt");
        let library = OverlayLibrary::new(dir.join("overlays"));
        let custom = dir.join("custom.html");
        std::fs::write(&custom, "<head></head>custom").unwrap();

        let target = library.adopt(&custom, false).unwrap().unwrap();
        assert_eq!(target, dir.join("overlays/custom.html"));
        assert!(custom.exists());
        assert_eq!(library.adopt(&custom, false).unwrap(), None);
        assert!(library.get("custom").is_some());
    }
}
