# Packaging and releases

The Makefile installs everything; these only wrap what it produced. That is
on purpose: a packaging script that lists the files again is a second
description of the layout, and two descriptions drift.

| | |
|---|---|
| `deb/control`, `deb/copyright` | metadata for the Debian binary package |
| `rpm/face-unlock.spec` | the RPM spec |
| `build-deb.sh`, `build-rpm.sh`, `build-tarball.sh` | build one package into `dist/` |
| `build-opencv.sh` | builds the static OpenCV the Arch tarball links in |
| `check-version.sh` | refuses a tag that disagrees with the Makefile |
| `publish-repos.sh` | regenerates the APT and RPM repositories |
| `pages/` | the landing page and the `.repo` file served from GitHub Pages |

The AUR packages live in [Felitendo/PKGBUILDS](https://github.com/Felitendo/PKGBUILDS):
`face-unlock` builds from source, `face-unlock-bin` takes the Arch tarball.
Its CI notices a new GitHub release, updates the checksums and pushes to the AUR.

Unlike the shell-only LoonixTools, this one is compiled. The packages are per
architecture (amd64 and x86_64), and they need the Qt 6 and KDE Frameworks 6
development packages to build: Debian 13 (trixie) and current Fedora have
them. The Debian package's library dependencies are read off the binaries by
`dpkg-shlibdeps`; RPM does the same on its own.

The program uses Qt's private API, so a package only fits the Qt it was built
against. The `.deb` is therefore built twice, in Debian 13 and in Ubuntu 26.04
(for Ubuntu and Kubuntu), with a suffix on the version (`~deb13`, `~ubuntu26.04`), and each
gets an APT repository of its own: `deb/trixie` and `deb/resolute`. The RPM is
built on the current Fedora.

The Arch tarball (`face-unlock-<version>-arch-x86_64.tar.zst`) is built on Arch,
for the same reason. It holds a folder with the `make install` tree. Arch
changes the OpenCV soname with every new OpenCV, so OpenCV is linked in
statically (`build-opencv.sh`: only the modules the daemon uses, nothing it
would load at run time). Qt stays shared, so a new Qt minor version on Arch
needs a new release.

The two networks (YuNet and SFace, from the OpenCV model zoo) are not in the
repository. `make models` downloads them and checks them against the
checksums in the Makefile. The RPM spec and the PKGBUILD list them as sources
of their own, with the same checksums.

Neither the deb nor the rpm switches anything on at install time. The daemon's
socket is enabled by `face-unlock` the first time somebody turns it on,
the agent is a user service each user enables, and the PAM files are only
touched when somebody asks for sudo or admin prompts. Removing a package
disables the socket.

## Building one by hand

```bash
packaging/build-deb.sh      # in a Debian 13 container, with the -dev packages from release.yml
packaging/build-rpm.sh      # in a Fedora container, with the -devel packages from the spec
packaging/build-opencv.sh && packaging/build-tarball.sh   # in an Arch container, with the packages from release.yml
```

All three take the version from `make version` unless one is passed as the first
argument.

## Making a release

1. Bump `VERSION` in the Makefile. The compiled programs get it from there
   too (`-DFU_VERSION`).
2. Add the release to `CHANGELOG.md`, in the format CLAUDE.md describes.
3. Commit, then `git tag vX.Y.Z && git push --tags`.

The `release` workflow builds the packages in Debian, Ubuntu, Fedora and Arch
containers, refuses the tag if it disagrees with the Makefile or has no
changelog entry, attaches the packages to a GitHub release with the entry as
its notes, and adds the deb and rpm files to the APT and RPM repositories on
the `gh-pages` branch.

## Trying the release path first

```bash
gh workflow run release.yml -f dry_run=true
```

Builds the packages, builds both repositories with a key generated on the
spot, checks the signatures, and installs the packages back out of the
repositories. Nothing is pushed and no release is made.

## Setting up the signing, once

```bash
gpg --batch --passphrase '' --quick-generate-key \
    'face-unlock repository <felitendoyt@gmail.com>' rsa4096 sign never

gpg --armor --export-secret-keys 'face-unlock repository' \
    | gh secret set GPG_PRIVATE_KEY
```

Without the secret the workflow still builds the packages and attaches them to
the release; it says so in the log and leaves the repositories alone.

## Pointing Pages at it, once, in this order

1. Set the secret, above.
2. Tag a release. The workflow creates the `gh-pages` branch and fills it.
3. *Then* set **Pages** to deploy from a branch and pick `gh-pages` at the
   root.
