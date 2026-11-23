# Built with `packaging/build-rpm.sh`, which passes the version in rather than
# editing this file: Cargo.toml is where the version is written down.
%global upstream_version %{?_version}%{!?_version:3.0.0}

Name:           face-unlock
Version:        %{upstream_version}
Release:        1%{?dist}
Summary:        Face unlock for Plasma, GNOME, Hyprland and Niri

# The program is GPL; the two networks it ships are MIT (YuNet) and
# Apache-2.0 (SFace).
License:        GPL-3.0-or-later AND MIT AND Apache-2.0
URL:            https://github.com/LoonixTools/face-unlock
Source0:        %{name}-%{version}.tar.gz
Source1:        https://github.com/opencv/opencv_zoo/raw/main/models/face_detection_yunet/face_detection_yunet_2023mar.onnx
Source2:        https://github.com/opencv/opencv_zoo/raw/main/models/face_recognition_sface/face_recognition_sface_2021dec.onnx

BuildRequires:  cargo
BuildRequires:  rust
BuildRequires:  gcc
BuildRequires:  make
BuildRequires:  scdoc
BuildRequires:  systemd-rpm-macros
BuildRequires:  pkgconfig(systemd)
BuildRequires:  pkgconfig(xkbcommon)
BuildRequires:  pam-devel

Requires:       polkit
Requires:       systemd
# What the window opens at run time.
Requires:       libwayland-client
Requires:       libxkbcommon
Requires:       fontconfig
Requires:       vulkan-loader
Recommends:     mesa-vulkan-drivers
# The old name.
Obsoletes:      plasma-face-unlock < 2
Provides:       plasma-face-unlock = %{version}-%{release}

%description
Look at the screen and it unlocks, the way a phone does. The lock screen, sudo
in a terminal and the admin password prompts can take a face instead of a
password, on KDE Plasma, GNOME, Hyprland and Niri. A bubble at the top of the
screen shows the face being looked for, recognised or refused.

This package was called plasma-face-unlock before. It takes over its faces and
settings.

A photo on a phone, a tablet or a glossy print is refused by its reflection and
its straight edges. The strict photo check also wants a sign of life (a blink,
or the nose moving the way a real nose does when the head turns), which stops a
printed photo too. It is a convenience, not a security upgrade: a webcam sees a
flat picture, and a video of the person can get through.

Run "face-unlock disable" before removing this package, so that sudo
and polkit go back to the password alone.

%prep
%autosetup -n %{name}-%{version}
mkdir -p models
cp %{SOURCE1} %{SOURCE2} models/

%build
%set_build_flags
make build PREFIX=%{_prefix} PAMDIR=%{_libdir}/security

%install
make install DESTDIR=%{buildroot} PREFIX=%{_prefix} \
	PAMDIR=%{_libdir}/security SYSTEMUNITDIR=%{_unitdir} USERUNITDIR=%{_userunitdir}

%preun
%systemd_preun face-unlockd.socket face-unlockd.service

%postun
%systemd_postun face-unlockd.socket face-unlockd.service

# After the old package is gone: its faces, settings and PAM lines.
%posttrans
%{_bindir}/face-unlock --root migrate || :

%files
%license LICENSE
%doc %{_datadir}/doc/%{name}/README.md
%{_bindir}/%{name}
%{_bindir}/%{name}-gui
%{_prefix}/lib/%{name}/
%{_datadir}/%{name}/
%{_libdir}/security/pam_face_unlock.so
%{_unitdir}/face-unlockd.socket
%{_unitdir}/face-unlockd.service
%{_userunitdir}/face-unlock-agent.service
%{_datadir}/applications/%{name}.desktop
%{_datadir}/applications/io.github.loonixtools.face-unlock-agent.desktop
%{_datadir}/polkit-1/actions/io.github.loonixtools.face-unlock.policy
%{_datadir}/icons/hicolor/scalable/apps/face-unlock.svg
%{_datadir}/gnome-shell/extensions/face-unlock@loonixtools.github.io/
%{_mandir}/man1/%{name}.1*

%changelog
