# TFC StreamManager (Linux)

A Linux desktop application for managing streaming overlays for table football (foosball) tournaments. Integrates with the [Kickertool](https://tournament.io) tournament management system to provide real-time match data for streaming platforms like OBS.

> **Looking for Windows?** Check out the [Windows version](https://github.com/beanieboi/TFCStreamManager-Windows) built with C#/WPF.

## Features

- **Three Operating Modes:**
  - **Kickertool Mode** - Automatically pulls match data from the Kickertool API with configurable refresh intervals
  - **Remote Mode** - Accepts score updates via HTTP POST requests (JSON)
  - **Manual Mode** - Direct entry of match information for non-tournament scenarios

- **Web Server** - Serves customizable HTML overlays (default port 8080)
- **mDNS/Bonjour Discovery** - Automatically advertises the service on your local network as "TFCStream"
- **Secure API Key Storage** - Uses the system keyring (Secret Service) with encrypted file fallback
- **Customizable Templates** - HTML templates with placeholder substitution for full overlay control
- **Debug Logging** - Built-in debug window for troubleshooting

## Requirements

- Linux with GTK4 libraries

For building from source, see [DEVELOPMENT.md](DEVELOPMENT.md).

## Usage

1. **Start the application** and click "Start Server"
2. **Select a mode:**
   - **Kickertool**: Enter your API key in Settings, then select a tournament and table
   - **Remote**: Send POST requests to `/scores` endpoint
   - **Manual**: Enter match details directly
3. **Add the overlay to OBS** as a Browser Source pointing to `http://localhost:8080`

### OBS Browser Source Settings

- **URL:** `http://localhost:8080` (or your configured port)
- **Width:** 1920 (recommended)
- **Height:** 1080 (recommended)
- **Custom CSS:** Leave empty or customize as needed

### Remote Mode API

Send POST requests to `/scores` with JSON:

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

## Configuration

Settings are stored in `~/.config/TFCStreamManager/`:
- `settings.json` - Application settings (port, refresh interval, etc.)

### Custom Overlay Templates

Place your custom `player_overlay.html` in:
1. `~/.config/TFCStreamManager/` (preferred)
2. Same directory as the executable
3. Current working directory

Available template placeholders:
- `{{tournamentName}}`, `{{table}}`
- `{{teamA}}`, `{{teamB}}`
- `{{teamAPlayer}}`, `{{teamBPlayer}}`
- `{{scoreA}}`, `{{scoreB}}`, `{{scoreName}}`
- `{{setsA}}`, `{{setsB}}`, `{{setsName}}`
- `{{roundName}}`, `{{groupName}}`, `{{disciplineName}}`
- `{{state}}`, `{{started}}`
- `{{refreshInterval}}`

## License

MIT License - see [LICENSE](LICENSE) for details.

## Related Projects

- [TFCStreamManager (Windows)](https://github.com/beanieboi/TFCStreamManager) - Windows version built with C#/WPF
- [Kickertool](https://tournament.io) - Tournament management platform
