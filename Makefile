# face-unlock: build and install
#
# Built with cargo: the command (face-unlock), the window (face-unlock-gui),
# the daemon (face-unlockd), the agent in the session (face-unlock-agent) and
# the PAM module. The translations in po/ are compiled into them. The install
# folders are handed to cargo, so every part agrees on where things are. The
# man page needs scdoc and is skipped without it.

# The version comes from Cargo.toml. VERSION is still taken, so a packager
# that passes it keeps working.
VERSION      ?= $(shell sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)

PREFIX       ?= /usr
DESTDIR      ?=
BINDIR       ?= $(PREFIX)/bin
DATADIR      ?= $(PREFIX)/share
LIBEXECDIR   ?= $(PREFIX)/lib/face-unlock
MODELDIR     ?= $(DATADIR)/face-unlock/models
MANDIR       ?= $(DATADIR)/man
APPDIR       ?= $(DATADIR)/applications
POLKITDIR    ?= $(DATADIR)/polkit-1/actions
ICONDIR      ?= $(DATADIR)/icons/hicolor/scalable/apps
GNOMEEXTDIR  ?= $(DATADIR)/gnome-shell/extensions
GNOMEEXT     := face-unlock@loonixtools.github.io

# Where systemd looks for units, asked of systemd itself for a normal install.
# A build with a prefix of its own keeps them under that prefix.
ifeq ($(PREFIX),/usr)
SYSTEMUNITDIR ?= $(shell pkg-config --variable=systemdsystemunitdir systemd 2>/dev/null || echo /usr/lib/systemd/system)
USERUNITDIR   ?= $(shell pkg-config --variable=systemduserunitdir systemd 2>/dev/null || echo /usr/lib/systemd/user)
else
SYSTEMUNITDIR ?= $(PREFIX)/lib/systemd/system
USERUNITDIR   ?= $(DATADIR)/systemd/user
endif

# Where PAM loads modules from. Not the same everywhere (/usr/lib/security on
# Arch, /usr/lib64/security on Fedora, a multiarch directory on Debian), and
# not something PAM itself answers, so it is found by looking for pam_unix.
PAMDIR       ?= $(shell for d in /usr/lib/security /usr/lib64/security /usr/lib/$$(gcc -dumpmachine 2>/dev/null)/security /lib/$$(gcc -dumpmachine 2>/dev/null)/security /lib/security; do [ -e $$d/pam_unix.so ] && { echo $$d; break; }; done)
ifeq ($(PAMDIR),)
PAMDIR       := /usr/lib/security
endif

CARGO        ?= cargo
CARGOFLAGS   ?= --release --locked
TARGET       := target/release

MANPAGE      := doc/face-unlock.1
SCDOC        := $(shell command -v scdoc 2>/dev/null)

# The two networks, from the OpenCV model zoo. YuNet (MIT) finds faces,
# SFace (Apache-2.0) recognises them. Pinned by checksum: a model is code as
# far as trust goes.
MODEL_BASE   := https://github.com/opencv/opencv_zoo/raw/main/models
MODELS       := face_detection_yunet_2023mar.onnx face_recognition_sface_2021dec.onnx
MODEL_URL_face_detection_yunet_2023mar.onnx   := $(MODEL_BASE)/face_detection_yunet/face_detection_yunet_2023mar.onnx
MODEL_SUM_face_detection_yunet_2023mar.onnx   := 8f2383e4dd3cfbb4553ea8718107fc0423210dc964f9f4280604804ed2552fa4
MODEL_URL_face_recognition_sface_2021dec.onnx := $(MODEL_BASE)/face_recognition_sface/face_recognition_sface_2021dec.onnx
MODEL_SUM_face_recognition_sface_2021dec.onnx := 0ba9fbfa01b5270c96627c4ef784da859931e02f04419c829e83484087c34e79

# A packager with the models already downloaded (an AUR source array, a
# build without network) points this at them.
MODELS_SRC   ?= models

# What the programs are told about the install, at build time.
export FU_LIBEXECDIR := $(LIBEXECDIR)
export FU_MODELDIR   := $(MODELDIR)
export FU_PAMDIR     := $(PAMDIR)
export FU_BINDIR     := $(BINDIR)

.PHONY: all build models check test install uninstall clean version

all: build

build: $(MANPAGE)
	$(CARGO) build $(CARGOFLAGS) --workspace

# The one place the version is written down, for everything that has to agree
# with it: the packaging scripts, and the release workflow checking that the
# tag it was handed says the same thing.
version:
	@echo $(VERSION)

models: $(addprefix models/,$(MODELS))

models/%.onnx:
	@mkdir -p models
	curl -fL --retry 3 -o $@.part "$(MODEL_URL_$*.onnx)"
	@echo "$(MODEL_SUM_$*.onnx)  $@.part" | sha256sum -c --quiet - || { rm -f $@.part; echo "checksum mismatch for $@" >&2; exit 1; }
	mv $@.part $@

$(MANPAGE): doc/face-unlock.1.scd
ifdef SCDOC
	$(SCDOC) < $< > $@
else
	@echo "scdoc not found, skipping $@"
endif

# The liveness cues against synthetic heads and photographs, the face store,
# the PAM file editing against copies of real PAM files, the settings files
# and the translations. None of it needs a camera, root or the models.
test:
	$(CARGO) test --workspace

check: test
	$(CARGO) clippy --workspace --all-targets -- -D warnings
	@if command -v desktop-file-validate >/dev/null 2>&1; then \
		desktop-file-validate res/applications/*.desktop && echo "ok  desktop-file-validate"; \
	fi

install:
	@for m in $(MODELS); do \
		[ -f "$(MODELS_SRC)/$$m" ] || { echo "$(MODELS_SRC)/$$m is missing: run 'make models' first" >&2; exit 1; }; \
	done

	# the command and the window
	install -Dm755 $(TARGET)/face-unlock     "$(DESTDIR)$(BINDIR)/face-unlock"
	install -Dm755 $(TARGET)/face-unlock-gui "$(DESTDIR)$(BINDIR)/face-unlock-gui"
	# the daemon, the agent and the PAM module
	install -Dm755 $(TARGET)/face-unlockd       "$(DESTDIR)$(LIBEXECDIR)/face-unlockd"
	install -Dm755 $(TARGET)/face-unlock-agent  "$(DESTDIR)$(LIBEXECDIR)/face-unlock-agent"
	install -Dm755 $(TARGET)/libpam_face_unlock.so "$(DESTDIR)$(PAMDIR)/pam_face_unlock.so"

	# the models
	@for m in $(MODELS); do \
		install -Dm644 "$(MODELS_SRC)/$$m" "$(DESTDIR)$(MODELDIR)/$$m"; \
	done

	# the daemon's socket and service, the agent's user service
	install -Dm644 res/systemd/face-unlockd.socket "$(DESTDIR)$(SYSTEMUNITDIR)/face-unlockd.socket"
	install -Dm644 res/systemd/face-unlockd.service "$(DESTDIR)$(SYSTEMUNITDIR)/face-unlockd.service"
	install -Dm644 res/systemd/face-unlock-agent.service "$(DESTDIR)$(USERUNITDIR)/face-unlock-agent.service"
	sed -i -e 's|@LIBEXECDIR@|$(LIBEXECDIR)|g' \
		"$(DESTDIR)$(SYSTEMUNITDIR)/face-unlockd.service" \
		"$(DESTDIR)$(USERUNITDIR)/face-unlock-agent.service"

	# the window in the application menu, the desktop file KWin looks for
	# before it lets the bubble above the lock screen, the polkit action, the
	# icon
	install -Dm644 res/applications/face-unlock.desktop "$(DESTDIR)$(APPDIR)/face-unlock.desktop"
	install -Dm644 res/applications/io.github.loonixtools.face-unlock-agent.desktop \
		"$(DESTDIR)$(APPDIR)/io.github.loonixtools.face-unlock-agent.desktop"
	sed -i -e 's|@LIBEXECDIR@|$(LIBEXECDIR)|g' "$(DESTDIR)$(APPDIR)/io.github.loonixtools.face-unlock-agent.desktop"
	install -Dm644 res/polkit/io.github.loonixtools.face-unlock.policy \
		"$(DESTDIR)$(POLKITDIR)/io.github.loonixtools.face-unlock.policy"
	sed -i -e 's|@BINDIR@|$(BINDIR)|g' "$(DESTDIR)$(POLKITDIR)/io.github.loonixtools.face-unlock.policy"
	install -Dm644 res/face-unlock.svg "$(DESTDIR)$(ICONDIR)/face-unlock.svg"

	# the GNOME Shell extension that draws the bubble on GNOME
	install -Dm644 -t "$(DESTDIR)$(GNOMEEXTDIR)/$(GNOMEEXT)" res/gnome-shell/$(GNOMEEXT)/*

	# documentation
	@if [ -f $(MANPAGE) ]; then \
		install -Dm644 $(MANPAGE) "$(DESTDIR)$(MANDIR)/man1/face-unlock.1"; \
	fi
	install -Dm644 README.md "$(DESTDIR)$(DATADIR)/doc/face-unlock/README.md"

uninstall:
	rm -f  "$(DESTDIR)$(BINDIR)/face-unlock"
	rm -f  "$(DESTDIR)$(BINDIR)/face-unlock-gui"
	rm -rf "$(DESTDIR)$(DATADIR)/face-unlock"
	rm -rf "$(DESTDIR)$(LIBEXECDIR)"
	rm -f  "$(DESTDIR)$(PAMDIR)/pam_face_unlock.so"
	rm -f  "$(DESTDIR)$(SYSTEMUNITDIR)/face-unlockd.socket"
	rm -f  "$(DESTDIR)$(SYSTEMUNITDIR)/face-unlockd.service"
	rm -f  "$(DESTDIR)$(USERUNITDIR)/face-unlock-agent.service"
	rm -f  "$(DESTDIR)$(APPDIR)/face-unlock.desktop"
	rm -f  "$(DESTDIR)$(APPDIR)/io.github.loonixtools.face-unlock-agent.desktop"
	rm -f  "$(DESTDIR)$(POLKITDIR)/io.github.loonixtools.face-unlock.policy"
	rm -f  "$(DESTDIR)$(ICONDIR)/face-unlock.svg"
	rm -rf "$(DESTDIR)$(GNOMEEXTDIR)/$(GNOMEEXT)"
	rm -f  "$(DESTDIR)$(MANDIR)/man1/face-unlock.1"
	rm -rf "$(DESTDIR)$(DATADIR)/doc/face-unlock"

clean:
	$(CARGO) clean
	rm -f $(MANPAGE)
