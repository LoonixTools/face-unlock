# Built with `packaging/build-rpm.sh`, which passes the version in rather than
# editing this file: the Makefile is where the version is written down.
%global upstream_version %{?_version}%{!?_version:1.0.0}

Name:           plasma-face-unlock
Version:        %{upstream_version}
Release:        1%{?dist}
Summary:        Face unlock for KDE Plasma

# The program is GPL; the two networks it ships are MIT (YuNet) and
# Apache-2.0 (SFace).
License:        GPL-3.0-or-later AND MIT AND Apache-2.0
URL:            https://github.com/LoonixTools/plasma-face-unlock
Source0:        %{name}-%{version}.tar.gz
Source1:        https://github.com/opencv/opencv_zoo/raw/main/models/face_detection_yunet/face_detection_yunet_2023mar.onnx
Source2:        https://github.com/opencv/opencv_zoo/raw/main/models/face_recognition_sface/face_recognition_sface_2021dec.onnx

BuildRequires:  cmake
BuildRequires:  gcc-c++
BuildRequires:  make
BuildRequires:  gettext
BuildRequires:  scdoc
BuildRequires:  systemd-rpm-macros
BuildRequires:  pkgconfig(systemd)
BuildRequires:  pkgconfig(libsystemd)
BuildRequires:  pam-devel
BuildRequires:  opencv-devel
BuildRequires:  cmake(Qt6Core)
BuildRequires:  cmake(Qt6DBus)
BuildRequires:  cmake(Qt6Network)
BuildRequires:  cmake(Qt6Gui)
BuildRequires:  cmake(Qt6Quick)
BuildRequires:  cmake(Qt6WaylandClient)
BuildRequires:  qt6-qtbase-private-devel
BuildRequires:  qt6-qtwayland-devel
BuildRequires:  cmake(LayerShellQt)
BuildRequires:  cmake(KF6I18n)

Requires:       bash >= 4.2
Requires:       coreutils
Requires:       gawk
Requires:       grep
Requires:       sed
Requires:       polkit
Requires:       systemd
Requires:       qt6-qtdeclarative
Recommends:     /usr/bin/gettext
Recommends:     kscreenlocker

%description
Look at the screen and it unlocks, the way a phone does. The lock screen, sudo
in a terminal and the admin password prompts of Plasma can take a face instead
of a password. A bubble at the top of the screen, above the lock screen too,
shows the face being looked for, recognised or refused.

Before a face counts it has to show a sign of life (a blink, or the nose moving
the way a real nose does when the head turns), so a photo held up to the camera
is not enough. It is a convenience, not a security upgrade: a webcam sees a
flat picture, and a video of the person can get through.

Run "plasma-face-unlock disable" before removing this package, so that sudo
and polkit go back to the password alone.

%prep
%autosetup -n %{name}-%{version}
mkdir -p models
cp %{SOURCE1} %{SOURCE2} models/

%build
%set_build_flags
make VERSION=%{upstream_version} PREFIX=%{_prefix} PAMDIR=%{_libdir}/security

%install
make install DESTDIR=%{buildroot} PREFIX=%{_prefix} VERSION=%{upstream_version} \
	PAMDIR=%{_libdir}/security SYSTEMUNITDIR=%{_unitdir} USERUNITDIR=%{_userunitdir}

%find_lang %{name}

%preun
%systemd_preun plasma-face-unlockd.socket plasma-face-unlockd.service

%postun
%systemd_postun plasma-face-unlockd.socket plasma-face-unlockd.service

%files -f %{name}.lang
%license LICENSE
%doc %{_datadir}/doc/%{name}/README.md
%{_bindir}/%{name}
%{_prefix}/lib/%{name}/
%{_datadir}/%{name}/
%{_libdir}/security/pam_plasma_face_unlock.so
%{_unitdir}/plasma-face-unlockd.socket
%{_unitdir}/plasma-face-unlockd.service
%{_userunitdir}/plasma-face-unlock-agent.service
%{_datadir}/applications/io.github.loonixtools.plasma-face-unlock-agent.desktop
%{_datadir}/polkit-1/actions/io.github.loonixtools.plasma-face-unlock.policy
%{_datadir}/icons/hicolor/scalable/apps/plasma-face-unlock.svg
%{_mandir}/man1/%{name}.1*

%changelog
