# CLAUDE.md

## Style

- Keep everything short: replies, explanations, comments, docs.
- Use simple English: short sentences, common words.
- Avoid em dashes (—). Do not just swap them for "-" either. Rewrite the sentence instead, for
  example with a comma, a colon, brackets or two sentences.

## Commits

- English only.
- Conventional Commits prefix: `feat:`, `fix:`, `chore:`, `docs:`, `refactor:`, `ci:`, `build:`.
- Subject as short as possible. Imperative, lowercase, no trailing period.
- Body only when something genuinely cannot be inferred from the diff.
- Never add `Co-Authored-By`, "Generated with" or any other AI attribution to commits or PR descriptions.
- Do not commit or push before I have checked the changes locally and said they are fine. Then
  commit and push. Several attempts at the same thing make one commit, and attempts that did not
  work make none. Different things done in one session get a commit each. This also goes for
  releases and tags.

## Layout

A cargo workspace, like cachy-auto-update.

- `src`: the command (`face-unlock`) and the library every part shares: paths, settings files,
  translations, the way to the daemon, the PAM files.
- `vision`: camera, detection, recognition, liveness, face store. Used by the daemon and the tests.
- `daemon`: the root service. It owns the camera and the face data.
- `paint`: how the bubble and face-unlock's own lock screen look, drawn with tiny-skia. The agent puts
  them on screen.
- `agent`: the program in the session: the bubble, face-unlock's own lock screen, unlocking by face.
- `gui`: the window, drawn with Rune: the settings, the setup of a face, the first start.
- `pam`: the PAM module for sudo, admin prompts and lock screens.
- `res/gnome-shell`: the extension that draws the bubble on GNOME.

## Releases

Releases look like the ones of big projects such as Immich. `.github/release-notes.sh <tag>` builds
the notes: the entry from `CHANGELOG.md` (welcome and highlights), a support section, every commit
since the last release sorted by its prefix (`feat`, `fix`, `docs`, ...) with author and link, and
the full changelog link. So commit subjects end up in public: keep them clear.

1. Read `git log <last tag>..HEAD` and pick the version: only fixes → patch, something new → minor,
   something that breaks or needs the user to act → major.
2. Bump the version and add the entry at the top of `CHANGELOG.md`, in one commit (`chore: 1.4.0`).
3. Push, then push the tag: `git tag v1.4.0 && git push origin main v1.4.0`. The `release` workflow
   builds the packages and creates the release. It stops before building when the entry is missing.
4. PKGBUILDS picks up the new release for the AUR on its own.

A minor or major release (has `### Highlights`, gets a heading and the support section):

```markdown
## v1.4.0

_2026-09-24_

Welcome to face-unlock `v1.4.0`! One or two sentences on what this release is about.

<p align="center">
  <img width="480" alt="What the picture shows" src="https://raw.githubusercontent.com/LoonixTools/face-unlock/v1.4.0/<path>">
</p>

### 🚨 Breaking changes

- Only if there are any: what changed, and what the user has to do.

### Highlights

- First highlight, a few words
- Second highlight
```

A picture under the welcome is optional. To show the menu or another screen of the program, use a
real screenshot of it running in Konsole, in English. Never a text copy of the screen. The same goes
for the README. The highlights stay a plain list: no heading or text per highlight, the list of
commits explains the rest.

A patch release is just a sentence or two, for example: "A small patch. The menu no longer closes
when you press Enter." The list of commits follows on its own.

- Write for users: what they notice, not how the code does it. Friendly and simple.
- Commands and settings they type go in backticks, buttons and labels in bold.
- To change an old release: edit its entry, commit, then
  `gh release edit <tag> --title <tag> --notes "$(.github/release-notes.sh <tag>)"`.

## README

- New UI (a window, a screen, a menu, a setting, a notification) gets a screenshot in the README,
  next to the text about it. Like for releases: a real screenshot of it running, in English.
- When a screen changes, take its screenshot again. The README never shows an old one.

## Testing

Never test against the real setup: enrolling and the PAM files belong to the user's machine.

- `make test` runs the liveness cues against synthetic heads and photos, the face store, and the
  PAM file editing against copies of real PAM files. No camera, no root.
- `FU_SHOTS=DIR cargo test -p face-unlock-paint frames` draws the bubble's frames into DIR.
- The window's pictures for the README: `face-unlock-gui --snap SCENE --out FILE`, the scenes are
  in `gui/src/main.rs`.
- The bubble and the own lock screen run in nested compositors: `kwin_wayland --virtual` (no
  window), and niri inside it for ext-session-lock. `face-unlock-agent --demo` plays the bubble.
- Without a camera, point the daemon at pictures or a video: `Camera=images:<dir>` or
  `Camera=file:<video>` in the config passed to `--config`.
- A development daemon needs no root: `face-unlockd --socket $XDG_RUNTIME_DIR/fu/socket
  --state-dir DIR --config FILE --models DIR`. It skips polkit when it does not run as root. Point
  the other parts at it with `FU_SOCKET`.
- face-unlock's own lock screen checks the password with PAM. `FU_PAM_CONFDIR=DIR` points it at a
  `face-unlock-lock` file of its own (pam_permit, pam_deny), so no test counts as a wrong password
  for the real account.
- `make check` after every change. New strings: `po/update-pot.sh`, then translate them in every `po/*.po`.
