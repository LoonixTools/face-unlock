# Packaging and releases

The Makefile installs everything; these only wrap what it produced. That is
on purpose: a packaging script that lists the files again is a second
description of the layout, and two descriptions drift.

| | |
|---|---|
| `deb/control`, `deb/copyright` | metadata for the Debian binary package |
| `rpm/plasma-face-unlock.spec` | the RPM spec |
| `build-deb.sh`, `build-rpm.sh` | build one package into `dist/` |
| `check-version.sh` | refuses a tag that disagrees with the Makefile |
| `publish-repos.sh` | regenerates the APT and RPM repositories |
| `pages/` | the landing page and the `.repo` file served from GitHub Pages |

The AUR package lives in [Felitendo/PKGBUILDS](https://github.com/Felitendo/PKGBUILDS/tree/main/plasma-face-unlock).
Its CI notices a new GitHub release, updates the checksum and pushes to the AUR.

Unlike the shell-only LoonixTools, this one is compiled. The packages are per
architecture (amd64 and x86_64), and they need the Plasma 6 and Qt 6
development packages to build: Debian 13 (trixie) and current Fedora have
them. The Debian package's library dependencies are read off the binaries by
`dpkg-shlibdeps`; RPM does the same on its own.

The program uses Qt's private API, so a package only fits the Qt it was built
against. The `.deb` is therefore built twice, in Debian 13 and in Ubuntu 26.04
(for Kubuntu), with a suffix on the version (`~deb13`, `~ubuntu26.04`), and each
gets an APT repository of its own: `deb/trixie` and `deb/resolute`. The RPM is
built on the current Fedora.

The two networks (YuNet and SFace, from the OpenCV model zoo) are not in the
repository. `make models` downloads them and checks them against the
checksums in the Makefile. The RPM spec and the PKGBUILD list them as sources
of their own, with the same checksums.

Neither the deb nor the rpm switches anything on at install time. The daemon's
socket is enabled by `plasma-face-unlock` the first time somebody turns it on,
the agent is a user service each user enables, and the PAM files are only
touched when somebody asks for sudo or admin prompts. Removing a package
disables the socket.

## Building one by hand

```bash
packaging/build-deb.sh      # in a Debian 13 container, with the -dev packages from release.yml
packaging/build-rpm.sh      # in a Fedora container, with the -devel packages from the spec
```

Both take the version from `make version` unless one is passed as the first
argument.

## Making a release

1. Bump `VERSION` in the Makefile. The compiled programs get it from there
   too (`-DPFU_VERSION`).
2. Commit, then `git tag vX.Y.Z && git push --tags`.

The `release` workflow builds both packages in a Debian and a Fedora container,
refuses the tag if it disagrees with the Makefile, attaches the packages to a
GitHub release, and adds them to the APT and RPM repositories on the `gh-pages`
branch.

## Trying the release path first

```bash
gh workflow run release.yml -f dry_run=true
```

Builds both packages, builds both repositories with a key generated on the
spot, checks the signatures, and installs the packages back out of the
repositories. Nothing is pushed and no release is made.

## Setting up the signing, once

```bash
gpg --batch --passphrase '' --quick-generate-key \
    'plasma-face-unlock repository <felitendoyt@gmail.com>' rsa4096 sign never

gpg --armor --export-secret-keys 'plasma-face-unlock repository' \
    | gh secret set GPG_PRIVATE_KEY
```

Without the secret the workflow still builds both packages and attaches them to
the release; it says so in the log and leaves the repositories alone.

## Pointing Pages at it, once, in this order

1. Set the secret, above.
2. Tag a release. The workflow creates the `gh-pages` branch and fills it.
3. *Then* set **Pages** to deploy from a branch and pick `gh-pages` at the
   root.
