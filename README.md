<p align="right"><b>English</b> · <a href="README.fr.md">Français</a></p>

<p align="center">
  <img src="docs/readme/titre.svg" alt="SpaceFiles, drawn in dots" width="854">
</p>

<p align="center">
  <b>A file explorer for Windows 11 that is drawn dot by dot.</b><br>
  Rust, raw Win32, Direct2D. No web view, no UI framework, no telemetry.
</p>

<p align="center">
  <code>3.70 MB exe</code> · <code>16 MB resident</code> · <code>594 tests</code> · <code>0 web views</code>
</p>

---

> **STATE ▸ IN DEVELOPMENT.** There is no release yet and no binary to download. Everything below exists in `main`
> today; what is missing is listed under [KNOWN LIMITS ▸](#known-limits-). If a number is on this page, it was measured.

## OPEN ▸

SpaceFiles is one borderless black window, one dot-matrix font (Doto), and menus that shout politely in capitals.
It is the file explorer of the [Space suite](https://github.com/nicoo-o): small, native, private Windows apps that
work together.

It was called *Nothing Explorer* until November 2026. The old protocol name still answers, like a cat.

## VIEW ▸

| | |
|---|---|
| **Tabs** | <kbd>Ctrl</kbd>+<kbd>T</kbd> / <kbd>W</kbd> / <kbd>Tab</kbd>, sessions saved 2 s after any change and reopened after a crash |
| **Dual pane** | <kbd>F6</kbd>, compare and synchronise with <kbd>Ctrl</kbd>+<kbd>Shift</kbd>+<kbd>F6</kbd> |
| **Three views** | List, grid (<kbd>Ctrl</kbd>+wheel), Miller columns |
| **Quick look** | <kbd>Space</kbd> on anything, plus a preview pane |
| **Themes** | Dark, light, auto or system; reduced motion switches every animation off |
| **Pixel** | Two bars in the status bar. They change colour with the hour and close their eyes at night. Not a mascot: a status light |

## FIND ▸

- **Type to filter**, <kbd>Ctrl</kbd>+<kbd>F</kbd> to search below the current folder.
- **Home-made index** (<kbd>Ctrl</kbd>+<kbd>Shift</kbd>+<kbd>F</kbd>): about 4 min 30 s and 110 MB for 2.5 million items, crawled at low
  priority. Filters: `ext:` `taille>` `modif:` `dossier:` `dans:` `contenu:` (yes, in French; so is the whole app).
- **Command palette**: <kbd>Ctrl</kbd>+<kbd>K</kbd>. Editable path: <kbd>Ctrl</kbd>+<kbd>L</kbd>.

## DO ▸

- Copy, move and recycle go through Windows' own `IFileOperation`, so **undo works**: <kbd>Ctrl</kbd>+<kbd>Z</kbd> /
  <kbd>Ctrl</kbd>+<kbd>Y</kbd>, 50 steps deep.
- Operation queue with pause and resume, side-by-side conflict view, a 16-bar gauge that ends in
  `ANNULER · CTRL Z`.
- Batch rename with regular expressions, tags (<kbd>Ctrl</kbd>+<kbd>1</kbd>/<kbd>2</kbd>/<kbd>3</kbd>), pins, SHA-256, archives.
- Browser downloads are watched: partial `.crdownload` and `.part` files get their own pill, finished ones a
  `NOUVEAU` mark.
- <kbd>Win</kbd>+<kbd>E</kbd> opens SpaceFiles when the small resident process is running.

## MEASURE ▸

No adjectives. Numbers, with their limits.

| What | Measured | Note |
|---|---|---|
| Executable | 3.70 MB | release build, LTO, `panic = abort` |
| Resident process | 16 MB working set | the part that stays in memory for <kbd>Win</kbd>+<kbd>E</kbd> |
| Folder of 200,000 entries | listed in 0.9 s, 195 MB | one single measurement, not an average |
| Index of 2.5 million items | ~4 min 30 s, 110 MB | low-priority crawl |
| Tests | 594 passed, 0 failed, 7 ignored | `cargo test`, 11 November 2026 |
| Direct dependencies | 3 crates | `windows`, `windows-core`, `regex` |

## PLUG ▸

SpaceFiles talks to the other Space apps through named pipes restricted to the current user
(`Eco.<app>.<user>`, JSON lines), a manifest, and one shared settings file. No network port, no account.

- [**SpaceHub**](https://github.com/nicoo-o/SpaceHub): the meeting point of the suite. It lists the apps, their
  versions and their health, and holds the shared theme and motion settings that SpaceFiles follows.
- [**SpaceNotch**](https://github.com/nicoo-o/SpaceNotch): the Dynamic Island for Windows 11. It can ask SpaceFiles
  for file searches, and SpaceFiles sends it the progress of its operations. <kbd>Alt</kbd>+<kbd>Space</kbd> belongs to SpaceNotch; that is why the palette here is
  <kbd>Ctrl</kbd>+<kbd>K</kbd>.
- The contract is written in [`docs/ecosystem/protocol.md`](docs/ecosystem/protocol.md). The Hub is never
  required, and no app launches another one to work.

## BUILD ▸

Windows 11 and a Rust toolchain. There is nothing else to install.

```powershell
cargo build --release      # target\release\spacefiles.exe
cargo test                 # about half a minute
./installer.ps1            # copies the exe to %LOCALAPPDATA%\Programs\SpaceFiles, shortcut and icon
```

Read [`docs/installation.md`](docs/installation.md) before running the installer: it explains what is written
and how `desinstaller.ps1` removes it (your data is kept unless you ask otherwise).

## KNOWN LIMITS ▸

- **No release, no signed binary, no auto-update.** Build it yourself or wait.
- **French only.** Interface, comments and documentation are in French. The filters above are not a typo.
- **No screenshots on this page yet.** The real ones show the author's folders; clean ones are on the list.
- SFTP, WebDAV, NAS backup and MTP are frozen behind an `experimental` setting, off for a new profile.
- Not covered by automated checks: anything that needs a real mouse (hover, right-click menu, lasso), a second
  screen with another DPI, full screen.
- The full list, unsoftened, is at the end of [`CHANGELOG.md`](CHANGELOG.md).

## ABOUT ▸

Built by one person, with a lot of help from coding agents; [`AGENTS.md`](AGENTS.md) is the rulebook they follow,
and it is a decent tour of the code for humans too. The dotted title above is generated by
[`tools/readme/titre.mjs`](tools/readme/titre.mjs).

[MIT](LICENSE). The fonts in `assets/fonts` keep their own licences.

<p align="center"><sub>6 ELEMENTS · 0 WEB VIEWS · READ IN ONE SCROLL</sub></p>
