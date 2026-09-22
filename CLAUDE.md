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

## Layout

- `src/plasma-face-unlock` and `src/lib/*.sh`: the command and its menu, plain bash.
- `src/core`: camera, detection, recognition, liveness, face store. Used by the daemon and the tests.
- `src/daemon`: the root service. It owns the camera and the face data.
- `src/agent`: the Qt/QML program in the session: bubble, lock screen, setup window.
- `src/pam`: the PAM module for sudo and admin prompts.
- `src/ctl`: the client the bash code talks to the daemon with.

## Testing

Never test against the real setup: enrolling and the PAM files belong to the user's machine.

- `make test` runs the liveness cues against synthetic heads and photos, the face store, and the
  PAM file editing against copies of real PAM files. No camera, no root.
- Without a camera, point the daemon at pictures or a video: `Camera=images:<dir>` or
  `Camera=file:<video>` in the config passed to `--config`.
- A development daemon needs no root: `plasma-face-unlockd --socket $XDG_RUNTIME_DIR/pfu/socket
  --state-dir DIR --config FILE --models DIR`. It skips polkit when it does not run as root. Point
  the other parts at it with `PFU_SOCKET`.
- `make check` after every change. New strings: `po/update-pot.sh`, then translate them in `po/de.po`.
