# TFC StreamManager (Linux)

A Linux desktop application for managing streaming overlays for table football (foosball) tournaments. Integrates with the [Kickertool](https://tournament.io) tournament management system to provide real-time match data for streaming platforms like OBS.

> **Looking for Windows?** Check out the [Windows version](https://github.com/beanieboi/TFCStreamManager-Windows) built with C#/WPF.

## Features

- **One match on the table**, from one of three competitions (ADR 0012 in `ideas/tfc-stream-manager`):
  - **Kickertool** - tournament, round, group, discipline, teams and players from the Kickertool API
  - **MTFV** - a Begegnung of the day from mtfv.de; players are entered
  - **Frei** - everything entered by hand
- **Live scores** from the app or the tablet, for every competition: goals, undo, corrections, players

- **Web Server** - Serves customizable HTML overlays (default port 8080)
- **mDNS/Bonjour Discovery** - Automatically advertises the service on your local network as "TFCStream"
- **Secure API Key Storage** - Uses the system keyring (Secret Service)
- **Customizable Templates** - HTML and CSS with `{{placeholders}}`, updated live over SSE
- **Logging** - Everything is logged to a file; the "Debug" window shows its tail live

## Requirements

- Linux with GTK4 libraries

For building from source, see [DEVELOPMENT.md](DEVELOPMENT.md).

## Usage

1. **Start the application.** The web server starts with it; "Erneut versuchen" appears if the port is taken.
2. **Choose what is played** and put a match on the table:
   - **Kickertool**: enter your API key in Settings, then select a tournament and table. Each new match on that table replaces the match.
   - **MTFV**: choose the league (from the mtfv.de season overview, which switches to the next season on its own), then one of its Begegnungen, and put it on the table ("Auf den Tisch"). A race league shows "Race bis" and "Tore je Segment" (one set of values for the day, shared with Frei) and its games from mtfv.de, one per segment (D1 D2 E1 D3 E2 D4 D5, read only); a set league shows its games, e.g. "5 Spiele à 2 Sätze: D1 E1 D2 E2 D3".
   - **Frei**: choose Einzel, Doppel or Race and enter the labels; a race also has its team names, "Race bis", "Tore je Segment" and one game per segment: click a game to switch between Doppel and Einzel (numbered in order, D1 D2 E1 …; preset as in MTFV, a setting for the day). The format (sets by hand, or a race to "Race bis") and the overlay template follow the kind. The players of Einzel and Doppel are entered below, with the score, and are also the name of their side. "Auf den Tisch" puts these details on the table: a Frei match of the same kind keeps its score, anything else starts a new one at 0:0. "Neustart" always starts at 0:0.
3. **Score it** below, or from the tablet: "Tore" and "Sätze", each with +1 and −1 per side, and the players of the current game. Sets are always entered by hand, as set rules (overtime in the last set) vary; in a race they are disabled. Kickertool and Frei: "Sätze +1" starts the next set at 0:0. MTFV set leagues: "Sätze +1" ends the set (its goals stay), the game's next set follows, and after the game's sets "Nächstes Spiel" moves on to the next game; "Sätze −1" reopens the set finished last. MTFV races go to "Race bis".
4. **Pick the overlay template** in the dropdown next to the competition. Each competition preselects a fitting one; a template chosen by hand is remembered for that competition.
5. **Add the overlay to OBS** as a Browser Source pointing to `http://localhost:8080`. Always as a URL: a local-file source cannot reach the server.

### OBS Browser Source Settings

- **URL:** `http://localhost:8080` (or your configured port) follows the template selected in the app. `http://localhost:8080/overlay/<name>` always shows that one template, e.g. for a separate source per OBS scene. The dropdown's tooltip shows the URL of the selected template.
- **Width:** 1920 (recommended)
- **Height:** 1080 (recommended)
- **Custom CSS:** Leave empty or customize as needed

### Old tablet protocol

The current tablet app sends POST requests to `/scores` with JSON:

```json
{
  "teamAScore": 0,
  "teamBScore": 0,
  "teamAName": "Team A",
  "teamBName": "Team B",
  "teamAPlayer": "Player 1",
  "teamBPlayer": "Player 2",
  "eventName": "Tournament Name"
}
```

It is an adapter onto the match on the table: names become the team names and the tournament label, scores a correction, players the lineup of the current game. Without a match it starts a Frei match. It will be removed once the tablet uses the API below.

### Match API

The server owns the match on the table (see the ADRs in `ideas/tfc-stream-manager`). It is persisted to `table_match.json` in the config directory, so a restart resumes a running match.

| Method | Path | |
|--------|------|-|
| `GET` | `/api/mtfv/fixtures?date=YYYY-MM-DD` | Begegnungen on that day (default today) |
| `POST` | `/api/match` | Put a match on the table: an MTFV Begegnung from the season overview `{"veranstaltung_id": 72, "begegnung_id": 3186}`, or a Frei match `{"home": "A", "away": "B", "labels": {"tournament": "Cup"}, "singles": false, "format": {"kind": "sets"}}` (or `{"kind": "race", "target": 42}`) |
| `DELETE` | `/api/match` | Clear the table |
| `GET` | `/api/match` | Setup: teams, rosters, units, lineups, event log |
| `GET` | `/api/scoreboard` | Derived state with its `revision` |
| `GET` | `/api/events` | SSE for the tablet, event `scoreboard` with the current state on connect and on every change (`null` when none) |
| `POST` | `/api/intents` | `{"revision": 3, "type": "goal", "side": "home"}`; also `undo`, `select` (`"unit": 2`, sets only), `finish`, `correct` (`"home": 4, "away": 2`: goals of the current set, or the race totals), `set_won` (`"side": "home"`: ends the current set; single games start the next at 0:0, MTFV games keep its goals), `correct_sets` (`"home": 1, "away": 1`, single games), `select` (`"unit": n`: MTFV, the first set of the next game) |
| `PUT` | `/api/lineups` | `{"revision": 3, "side": "away", "slot": "D1", "players": ["123", "Max Muster"]}`: roster ids or typed names |
| `PUT` | `/api/setup` | `{"revision": 3, "labels": {"tournament": "Cup", "round": "Finale"}, "home": "A", "away": "B"}`, each optional |
| `PUT` | `/api/format` | `{"revision": 3, "format": {"kind": "race", "target": 10}}`, Kickertool and Frei only |

Every change carries the `revision` the client last saw. A stale one is answered with `409` and the current `scoreboard`. Invalid commands are `422`, and no match on the table is `404`.

## Logs

The app logs to `logs/tfc_stream_manager.log` in its data directory (`~/.local/state/TFCStreamManager/` on Linux, `~/Library/Application Support/com.benfritsch.TFCStreamManager/` on macOS) and to the terminal. "Debug" in the toolbar shows the end of the file and follows it, like `tail -f`. Above 5 MB the file is moved to `.log.1` at the next start.

Each line has the time, the launch id of the app start it belongs to (random, 6 hex digits, e.g. `103ff9`; `grep` for it to see one run), a level and the module it comes from:

```
2026-11-15 09:43:46 103ff9  INFO tfc_stream_manager::services::logging: App started: version 1.0.0, launch 103ff9, …
```

The debug window separates the runs with an "App-Start · Launch …" row.

| Level | For |
|---|---|
| `ERROR` | something failed the operator has to know about (server or mDNS not started, settings or the match not saved, Kickertool or MTFV data not loaded) |
| `WARN` | a fallback or a passing failure (a Kickertool poll, an unreadable file replaced by defaults, a skipped overlay, HTTP 4xx) |
| `INFO` | state changes (server, mDNS, competition, the match on the table, polling) |
| `DEBUG` | frequent detail (every HTTP request with status and duration, mDNS announcements per interface, refused commands, `/scores` content) |

The file always gets everything of the app (`DEBUG`) and warnings of its dependencies. `RUST_LOG` only sets the terminal (default `info`).

## Configuration

Settings are stored in `~/.config/TFCStreamManager/`:
- `settings.json` - Application settings
- `translations.toml` - Optional overrides for the overlay names of Kickertool rounds, groups and disciplines, and of MTFV games. The defaults (German) are in [`translations.toml`](translations.toml); copy only the entries you want to change. Read when a table is selected.

| Setting | Default | Description |
|---------|---------|-------------|
| `port` | `8080` | Web server port (1024–65535); an invalid entry in the settings keeps the old one |
| `refresh_interval` | `30` | Kickertool polling interval in seconds (1–300) |
| `competition` | `null` | `kickertool`, `mtfv` or `manual` (Frei), as chosen in the toolbar |
| `race_target` | `42` | Goals to win a race; a draw is one goal before it (MTFV) |
| `race_segment` | `6` | Goals per race segment; the segments follow (42 in sixes: 7). `0`: none |
| `race_games` | `DDEDEDD` | Frei races: the game of each segment, `D` (Doppel) or `E` (Einzel); missing segments are Doppel |
| `mtfv_league` | `null` | The MTFV league last chosen |
| `overlay_dir` | `null` | The overlay folder; `null` is `overlays/` in the config directory |
| `overlay_choices` | `{}` | Overlay template chosen by hand, per competition (set from the toolbar) |

### Overlay Templates

All templates come from one folder, the overlay folder: `overlays/` in the config directory unless another one is chosen in the settings ("Overlay-Ordner", "Wählen…"). No templates ship with the app; they are maintained separately, so none go stale inside it. An empty folder means no overlay.

The club's templates are kept in [`overlays/`](overlays) in this repository (Turnier Einzel, Turnier Doppel, MTFV Race 42, MTFV Sätze). Copy them into the overlay folder, or choose `overlays/` itself as the overlay folder to work on them.

Every request reads the folder again, so an edited, added or removed file is used right away. The server also checks the folder every second and tells open overlay pages (OBS included) to reload when a file changed. A template describes itself in its `<head>`:

```html
<meta name="tfc:name" content="Team">
<meta name="tfc:sources" content="kickertool manual">
```

`tfc:name` is shown in the dropdown (default: the file name). For MTFV the league's format picks among the templates with that `tfc:format` (choices by hand are remembered for race and sets separately). `tfc:sources` lists the competitions it fits: `kickertool`, `mtfv`, `manual` (Frei). Without it, it fits all.

`<meta name="tfc:format" content="race">` (or `sets`) makes a Kickertool or Frei match on air take that format: a race template turns it into a race to the "Race bis" target, and the Sätze input is disabled; a set template turns it back. MTFV formats come from the league.

`<meta name="tfc:players" content="singles">` (or `doubles`) says who a template is for. The template follows the match on the table: for Kickertool and Frei by its players per side (Kickertool sends them with each match; Frei has "Einzel"), for MTFV by the league's format. Only templates that fit are offered in the dropdown, and a choice by hand is remembered per competition and kind (`kickertool:singles`, `kickertool:doubles`, `manual:…`, `mtfv:race`, `mtfv:sets`).

An `overlay_path` from earlier versions is copied into the overlays folder once and removed from the settings.

#### Writing a template

A template is HTML and CSS, without a script of its own. The server adds `/overlay.js` ([`assets/overlay.js`](assets/overlay.js), part of the app) to every overlay page. It subscribes to `/api/overlay` (SSE: event `overlay` with the view of the match on the table, `null` when there is none) and fills every `{{placeholder}}` in text and attributes, as text. A missing value is empty.

| Placeholders | Filled by |
|---|---|
| `{{tournamentName}}`, `{{table}}`, `{{roundName}}`, `{{groupName}}`, `{{disciplineName}}` | labels, where known |
| `{{teamA}}`, `{{teamB}}`, `{{teamAPlayer}}`, `{{teamBPlayer}}` (players joined with ` / `), `{{teamAPlayer1}}`, `{{teamAPlayer2}}`, `{{teamBPlayer1}}`, `{{teamBPlayer2}}`, `{{scoreA}}`, `{{scoreB}}`, `{{setsA}}`, `{{setsB}}`, `{{state}}` (`running`, `finished`), `{{winner}}` | always |
| `{{matchPointsA}}`, `{{matchPointsB}}`, `{{unitNumber}}`, `{{unitCount}}`, `{{setNumber}}`, `{{setsInGame}}` | games and sets |
| `{{raceTarget}}`, `{{segmentNumber}}`, `{{segmentCount}}` | a race; segments only with goals per segment set |
| `{{slotA}}`, `{{slotB}}`, `{{gameName}}` | matches of several games (MTFV, Frei races): the current game, e.g. `D1`, "Einzel 1" (named by `[disciplines]` in `translations.toml`) |
| `{{started}}` | not filled yet |

Values are names and numbers; words like "Abschnitt" belong in the template. The script sets classes to style with:

| Class | On | When |
|---|---|---|
| `tfc-empty` | element | all placeholders inside it are empty |
| `tfc-changed` | element | its value just changed (600 ms), e.g. for a score animation |
| `tfc-idle` | `body` | nothing on air |
| `tfc-source-<competition>`, `tfc-format-race` / `tfc-format-sets` | `body` | while a match is on the table |

Pages at `/` reload when another template is selected. The Frei form disables the fields whose placeholder the template for Frei does not use.

## License

MIT License - see [LICENSE](LICENSE) for details.

## Related Projects

- [TFCStreamManager (Windows)](https://github.com/beanieboi/TFCStreamManager-Windows) - Windows version built with C#/WPF
- [Kickertool](https://tournament.io) - Tournament management platform
