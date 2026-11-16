# LOL Chess

```powershell
irm https://raw.githubusercontent.com/zallkyre/lol-chess/main/install.ps1 | iex
```

One line. Installs to `%LOCALAPPDATA%\LolChess`, fetches Stockfish, builds,
writes a **LOL Chess** shortcut to the Desktop and launches it.

Native Rust desktop chess trainer. Lichess board and pieces, lichess piece
movement, chess-coach grading.

- **Board & pieces**: lichess SVGs (`lichess/pieces/`), six sets, seven lichess
  board palettes. Click a palette with **Board**, a set with **Pieces**.
- **Movement**: exactly lichess. A *dragged* piece is under the cursor already,
  so it snaps on drop. A *click-click* move slides over 140 ms. Illegal drops
  snap back with no animation.
- **Engine**: Stockfish over UCI, spawned from `engine/stockfish.exe`, using one
  thread per core (capped at 8).
- **Ratings**: win-percent deltas, not pawns. Lenient bands:
  Excellent <=3, Good <=8, Inaccuracy <=14, Mistake <=26, Blunder above.
- **Tactics**: fork, pin, skewer, discovered attack, double check, back rank,
  removing the defender, sacrifice, promotion, checkmate. Named on screen.
- **Forced**: one legal move - the king is in check and it has to go there.
- **Coach**: local, rule-based, names the tactic. No chat with the bot.
- **Hopeless bot**: tick `hopeless bot` in the sidebar and the engine plays the
  *worst* move it can find: every legal move scored by Stockfish at depth 8 via a
  MultiPV-50 sweep, lowest score wins. Not random - deliberately worst.
- **Bots**: the **Bots** button opens one **opponent elo** slider, 400 to 3500 in
  steps of 100 - every step is its own bot. Below it you pick colour (white /
  black / random) and speed (bullet, blitz, rapid, classical, no clock), then a
  real game with an increment clock. Above 2700 the engine holds skill 20 and
  the search deepens with the slider (12 plies up to 18), so 3500 is genuinely
  harder than 2700. Elo moves like lichess (K=40 under 30 games, then 20); your
  rating and per-elo W/L/D persist in `profile.ini` next to the exe.
- **Auto-update check**: on launch the app compares `VERSION` against the repo
  and shows an **Update** button when they differ; the button runs the installer.

## Run

```powershell
.\run.ps1            # built exe if present, else cargo run --release
```

## Build

```powershell
cargo build --release
```

Then copy `target\release\lol-chess.exe` next to `engine\` and `lichess\` if you
want a portable folder.

## Shortcut

```powershell
.\make-shortcut.ps1  # writes "LOL Chess.lnk" to the Desktop
```

## Layout

```
src/main.rs     window, panels, coach, game flow
src/board.rs    board painting, input, drag/slide animation
src/engine.rs   UCI worker (blocking recv, backoff, reboot on crash)
src/grade.rs    ratings, tactics, attack map
src/assets.rs   lichess palettes, SVG rasterising, badges
lichess/        piece SVGs
engine/         stockfish.exe
assets/badges/  rating icons
_reference/     the original Python chess-coach (not compiled)
```

## Credit

Piece sets from [lichess](https://github.com/lichess-org/lila), each under its
own licence. Stockfish is GPLv3 and used unmodified.