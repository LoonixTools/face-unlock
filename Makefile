# face-unlock: build and install
#
# The shell front end installs as it is, like middleclick-autoscroll. The rest
# is compiled by CMake (the daemon, the agent, the PAM module, the client the
# shell code uses), which this Makefile drives, handing it every path so the
# two halves agree on where things are. Translations and the man page are
# optional and skipped when msgfmt or scdoc are missing.

# Overridable so a packager can pass the version it is actually building
# (`make VERSION=$pkgver`). The literal below is the fallback for builds
# straight from a checkout, and is what a release tag has to carry.
VERSION      ?= 1.0.1

PREFIX       ?= /usr
DESTDIR      ?=
BINDIR       ?= $(PREFIX)/bin
DATADIR      ?= $(PREFIX)/share
LIBDIR       ?= $(DATADIR)/face-unlock/lib
LIBEXECDIR   ?= $(PREFIX)/lib/face-unlock
MODELDIR     ?= $(DATADIR)/face-unlock/models
LOCALEDIR    ?= $(DATADIR)/locale
MANDIR       ?= $(DATADIR)/man
APPDIR       ?= $(DATADIR)/applications
POLKITDIR    ?= $(DATADIR)/polkit-1/actions
ICONDIR      ?= $(DATADIR)/icons/hicolor/scalable/apps

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

BUILDDIR     ?= build
CMAKE        ?= cmake
# One compiler per core. A bare --parallel lets make start them all at once,
# and a few dozen Qt and OpenCV files at once can use up the memory.
JOBS         ?= $(shell nproc 2>/dev/null || echo 2)
CMAKE_FLAGS  ?=

LINGUAS      := de es fr it ja ko nl pl pt_BR ru tr uk zh_CN
MOFILES      := $(patsubst %,po/%.mo,$(LINGUAS))
MANPAGE      := doc/face-unlock.1

LIBS         := $(wildcard src/lib/*.sh)

MSGFMT       := $(shell command -v msgfmt 2>/dev/null)
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

.PHONY: all build native models check test install uninstall clean version

all: build

build: native $(MOFILES) $(MANPAGE)

# The one place the version is written down, for everything that has to agree
# with it: the packaging scripts, and the release workflow checking that the
# tag it was handed says the same thing.
version:
	@echo $(VERSION)

native:
	$(CMAKE) -S . -B $(BUILDDIR) \
		-DCMAKE_BUILD_TYPE=Release \
		-DCMAKE_INSTALL_PREFIX=$(PREFIX) \
		-DFU_VERSION=$(VERSION) \
		-DFU_LIBEXECDIR=$(LIBEXECDIR) \
		-DFU_MODELDIR=$(MODELDIR) \
		-DFU_LOCALEDIR=$(LOCALEDIR) \
		-DFU_PAMDIR=$(PAMDIR) \
		$(CMAKE_FLAGS)
	$(CMAKE) --build $(BUILDDIR) --parallel $(JOBS)

models: $(addprefix models/,$(MODELS))

models/%.onnx:
	@mkdir -p models
	curl -fL --retry 3 -o $@.part "$(MODEL_URL_$*.onnx)"
	@echo "$(MODEL_SUM_$*.onnx)  $@.part" | sha256sum -c --quiet - || { rm -f $@.part; echo "checksum mismatch for $@" >&2; exit 1; }
	mv $@.part $@

po/%.mo: po/%.po
ifdef MSGFMT
	$(MSGFMT) --check --output-file=$@ $<
else
	@echo "msgfmt not found, skipping $@"
endif

$(MANPAGE): doc/face-unlock.1.scd
ifdef SCDOC
	$(SCDOC) < $< > $@
else
	@echo "scdoc not found, skipping $@"
endif

# Syntax-check every shell file, and run shellcheck when it is available.
check:
	@set -e; for f in src/face-unlock $(LIBS) tests/*.sh; do \
		bash -n "$$f" && echo "ok  $$f"; \
	done
	@if command -v shellcheck >/dev/null 2>&1; then \
		shellcheck -x -e SC1090,SC1091 src/face-unlock $(LIBS) tests/*.sh; \
		echo "ok  shellcheck"; \
	else \
		echo "shellcheck not found, skipped"; \
	fi
	@if command -v desktop-file-validate >/dev/null 2>&1; then \
		desktop-file-validate res/applications/*.desktop && echo "ok  desktop-file-validate"; \
	fi

# The liveness cues against synthetic heads and photographs, the face store,
# and the PAM file editing against copies of real PAM files. None of it needs
# a camera, root or the models.
test: native
	cd $(BUILDDIR) && ctest --output-on-failure
	bash tests/test_pam.sh

install: build
	@for m in $(MODELS); do \
		[ -f "$(MODELS_SRC)/$$m" ] || { echo "$(MODELS_SRC)/$$m is missing: run 'make models' first" >&2; exit 1; }; \
	done
	DESTDIR="$(DESTDIR)" $(CMAKE) --install $(BUILDDIR)

	# the command
	install -Dm755 src/face-unlock "$(DESTDIR)$(BINDIR)/face-unlock"
	install -d "$(DESTDIR)$(LIBDIR)"
	install -Dm644 -t "$(DESTDIR)$(LIBDIR)" $(LIBS)
	sed -i -e 's|@VERSION@|$(VERSION)|g' \
	       -e 's|@LIBDIR@|$(LIBDIR)|g' \
	       -e 's|@LIBEXECDIR@|$(LIBEXECDIR)|g' \
	       -e 's|@LOCALEDIR@|$(LOCALEDIR)|g' \
	       -e 's|@PAMDIR@|$(PAMDIR)|g' \
	       "$(DESTDIR)$(BINDIR)/face-unlock" \
	       "$(DESTDIR)$(LIBDIR)"/*.sh

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

	# the desktop file KWin looks for before it lets the bubble above the
	# lock screen, the polkit action, the icon
	install -Dm644 res/applications/io.github.loonixtools.face-unlock-agent.desktop \
		"$(DESTDIR)$(APPDIR)/io.github.loonixtools.face-unlock-agent.desktop"
	sed -i -e 's|@LIBEXECDIR@|$(LIBEXECDIR)|g' "$(DESTDIR)$(APPDIR)/io.github.loonixtools.face-unlock-agent.desktop"
	install -Dm644 res/polkit/io.github.loonixtools.face-unlock.policy \
		"$(DESTDIR)$(POLKITDIR)/io.github.loonixtools.face-unlock.policy"
	install -Dm644 res/face-unlock.svg "$(DESTDIR)$(ICONDIR)/face-unlock.svg"

	# translations
	@for l in $(LINGUAS); do \
		if [ -f "po/$$l.mo" ]; then \
			install -Dm644 "po/$$l.mo" \
				"$(DESTDIR)$(LOCALEDIR)/$$l/LC_MESSAGES/face-unlock.mo"; \
		fi; \
	done

	# documentation
	@if [ -f $(MANPAGE) ]; then \
		install -Dm644 $(MANPAGE) "$(DESTDIR)$(MANDIR)/man1/face-unlock.1"; \
	fi
	install -Dm644 README.md "$(DESTDIR)$(DATADIR)/doc/face-unlock/README.md"

uninstall:
	rm -f  "$(DESTDIR)$(BINDIR)/face-unlock"
	rm -rf "$(DESTDIR)$(DATADIR)/face-unlock"
	rm -rf "$(DESTDIR)$(LIBEXECDIR)"
	rm -f  "$(DESTDIR)$(PAMDIR)/pam_face_unlock.so"
	rm -f  "$(DESTDIR)$(SYSTEMUNITDIR)/face-unlockd.socket"
	rm -f  "$(DESTDIR)$(SYSTEMUNITDIR)/face-unlockd.service"
	rm -f  "$(DESTDIR)$(USERUNITDIR)/face-unlock-agent.service"
	rm -f  "$(DESTDIR)$(APPDIR)/io.github.loonixtools.face-unlock-agent.desktop"
	rm -f  "$(DESTDIR)$(POLKITDIR)/io.github.loonixtools.face-unlock.policy"
	rm -f  "$(DESTDIR)$(ICONDIR)/face-unlock.svg"
	rm -f  "$(DESTDIR)$(MANDIR)/man1/face-unlock.1"
	rm -rf "$(DESTDIR)$(DATADIR)/doc/face-unlock"
	@for l in $(LINGUAS); do rm -f "$(DESTDIR)$(LOCALEDIR)/$$l/LC_MESSAGES/face-unlock.mo"; done

clean:
	rm -rf $(BUILDDIR) po/*.mo $(MANPAGE)
