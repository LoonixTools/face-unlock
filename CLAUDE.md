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
  commit the result once and push. No commits for attempts that did not work. This also goes for
  releases and tags.

## Layout

- `src/face-unlock` and `src/lib/*.sh`: the command and its menu, plain bash.
- `src/core`: camera, detection, recognition, liveness, face store. Used by the daemon and the tests.
- `src/daemon`: the root service. It owns the camera and the face data.
- `src/agent`: the Qt/QML program in the session: bubble, lock screen, setup window.
- `src/pam`: the PAM module for sudo and admin prompts.
- `src/ctl`: the client the bash code talks to the daemon with.

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

## Testing

Never test against the real setup: enrolling and the PAM files belong to the user's machine.

- `make test` runs the liveness cues against synthetic heads and photos, the face store, and the
  PAM file editing against copies of real PAM files. No camera, no root.
- Without a camera, point the daemon at pictures or a video: `Camera=images:<dir>` or
  `Camera=file:<video>` in the config passed to `--config`.
- A development daemon needs no root: `face-unlockd --socket $XDG_RUNTIME_DIR/fu/socket
  --state-dir DIR --config FILE --models DIR`. It skips polkit when it does not run as root. Point
  the other parts at it with `FU_SOCKET`.
- `make check` after every change. New strings: `po/update-pot.sh`, then translate them in every `po/*.po`.
