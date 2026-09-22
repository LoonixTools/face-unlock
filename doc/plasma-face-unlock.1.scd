plasma-face-unlock(1)

# NAME

plasma-face-unlock - face unlock for KDE Plasma

# SYNOPSIS

*plasma-face-unlock* [_command_]

# DESCRIPTION

Look at the screen and it unlocks, the way a phone does. The lock screen, sudo
in a terminal and the admin password prompts of Plasma can all take a face
instead of a password. Before a face counts, it has to show a sign of life, so
holding up a photo of somebody is not enough.

A bubble at the top of the screen shows what is going on: it drops down when
the camera starts looking, the face in it looks around, and it turns into a
tick when it recognises somebody. It shows above the lock screen too.

Run without a command it shows an interactive menu. Everything it can be told
is reachable from there; the settings files behind it do not need to be edited
by hand.

It is a convenience, not a security upgrade. See *HOW SAFE IS THIS*.

# COMMANDS

*enable*
	Turn face unlock on for this user. Sets up a face first if there is none,
	starts the lock screen agent, and turns sudo and admin prompts back on if
	they were on before.

*disable*
	Turn it off: the agent stops and sudo and the admin prompts go back to the
	password alone. The faces are kept.

*setup* [_name_]
	Add a face. Opens the setup window: look at the camera, then move your head
	slowly in a circle until the ring is full. Adding a face asks for the
	password first.

*faces*
	List the faces, with the id *remove* takes.

*remove* _id_
	Delete a face.

*test*
	One scan, with what the checks see printed as it happens: how well the face
	matches, which way the head points, and how close each photo check is to
	firing.

*status*
	What is on, and when the last unlock was.

*-h*, *--help*
	Show a summary of the commands.

*-V*, *--version*
	Show the version.

# THE PIECES

*plasma-face-unlockd*
	The daemon, running as root and started by its socket
	(_plasma-face-unlockd.socket_). It is the only thing that opens the camera
	and the only thing that can read the face data. It exits after a minute
	with nothing to do.

*plasma-face-unlock-agent*
	Runs in the Plasma session as a user service
	(_plasma-face-unlock-agent.service_). It watches the lock screen, asks the
	daemon to scan when somebody comes back, unlocks the session when the face
	matches, draws the bubble, and is the setup window.

*pam_plasma_face_unlock.so*
	The PAM module for sudo and admin prompts. It asks the daemon and turns
	the answer into a PAM result.

*plasma-face-unlock-ctl*
	How this command talks to the daemon.

# RECOGNITION

Two small networks from the OpenCV model zoo do the work, on the CPU through
OpenCV's DNN module. YuNet finds the face and five points on it (the eyes, the
tip of the nose, the corners of the mouth). SFace turns an aligned crop of the
face into 128 numbers; two crops of the same person give numbers that point
the same way, and the match is how closely they do (cosine similarity).

Setting up a face stores a few of those numbers per head direction: straight
ahead, and eight directions around it. No picture is ever written anywhere.
With *Learn from every unlock* on, a confident unlock adds one more sample (at
most twelve per face, the oldest go first), so a new haircut or glasses do
not need a new setup. Face ID does the same. Only unlocks well above the
threshold count, so the samples cannot drift towards somebody else one
borderline unlock at a time.

A match has to hold for two frames. The face has to look at the screen with
its eyes open (*Only while looking at the screen*), and it has to be the same
face that shows the sign of life: a face that jumps in the picture, disappears
or stops matching starts the whole check again.

# HOW IT TELLS A FACE FROM A PHOTO

The *Photo check* follows the model Glance (the macOS face unlock) arrived at:
a few cues, each decisive on its own, in two kinds.

Deny cues are evidence of a fake and fail the scan straight away:

- *glare*: one large, flat, colourless highlight on the face, the way a
  phone screen or a glossy print throws back light. Skin shines in small
  scattered spots. The eyes are left out, because glasses reflect the screen.
- *edge*: the straight edges of a phone or tablet framing the face.

Confirm cues are evidence of a real head. *strict* needs one of them:

- *blink*: the dark of both eyes shrinks to a line and comes back within the
  fraction of a second a blink takes, while the rest of the face holds still
  and the light does not change.
- *head turn*: when the head turns, the eyes and the corners of the mouth
  (which lie close to one plane) predict exactly where a flat picture's nose
  would go. A real nose sits in front of that plane and misses the prediction
  by about as much as the head turned. The miss has to be consistent with a
  real turn, measured without the nose's own jitter, and the face has to
  narrow no more than a head that turned that far would.

*basic* uses the deny cues only. *off* checks nothing and is only for trying
out a camera.

# HOW SAFE IS THIS

A webcam sees a flat picture. A phone's face unlock builds a depth map with a
projector and an infrared camera; this cannot. What the photo check does:

- a photo, printed or on a screen, held up and turned any way, is refused by
  *strict*;
- a photo curled strongly and turned a lot can pass the head turn cue some of
  the time. Five points on a face cannot tell that from a very flat real face.
  Blink, glare and edge are what is left against it;
- a *video* of the person blinking or turning their head can pass the confirm
  cues. The deny cues catch a screen held up to the camera often, not always.

The other guards:

- five failed scans in a row with a face in view pause face unlock for
  fifteen minutes, or until the session is unlocked with the password;
- the face data is readable by root only, and adding, changing or deleting a
  face needs the password (polkit, *auth_self_keep*). A face cannot answer
  that question even with admin prompts on: the daemon refuses to scan while
  it is being asked;
- sudo and admin prompts are refused over SSH and for anybody without an
  active session at the machine;
- the lock screen is unlocked through logind, the same way
  *loginctl unlock-session* does it. That does not lower the bar: any program
  running as the user could already do that. For sudo and admin prompts the
  decision is the daemon's, which runs as root, and the PAM module only talks
  to a daemon that runs as root.

If that is not enough for what the machine guards, leave sudo and admin
prompts off, or face unlock altogether.

# THE LOCK SCREEN

Plasma's lock screen runs a fingerprint stack next to the password, but starts
it once per lock, gives up for good after the first failure and labels it for
fingerprints. So face unlock does not go through the lock screen's PAM at all.
The agent watches for the screen to lock (org.freedesktop.ScreenSaver) and
scans when somebody comes back:

- on any key or mouse movement after the screen locked, once 1.5 seconds have
  passed (the key that locked it does not count);
- when the machine wakes from sleep;
- right after locking, if *Look right after the screen locks* is on. Off by
  default: whoever locks their screen on purpose is usually still in front of
  it.

After a scan that did not get anybody in, the next one waits for the person to
be still for two seconds and then touch something again, so typing the password
does not start a scan with every key.

Unlocked through logind, the lock screen cuts off its own password prompt and
counts that as a wrong password. After a face unlock the daemon resets the
failed logins of *pam_faillock*(8), as a correct password does, so face unlocks
never lock the account.

The bubble is a layer-shell surface that KWin keeps above the lock screen
(kde_lockscreen_overlay_v1). KWin only allows that for a program whose desktop
file asks for it, which is
_io.github.loonixtools.plasma-face-unlock-agent.desktop_.

# SUDO AND ADMIN PROMPTS

Turned on under *Settings*, one line goes in front of the service's PAM stack:

```
-auth  sufficient  /usr/lib/security/pam_plasma_face_unlock.so
```

A match lets the person in; anything else falls through to the password as if
the line were not there. The dash makes PAM skip it quietly if the module ever
goes missing, so sudo keeps working even when the package was removed without
turning this off first.

Where _/etc/pam.d/<service>_ exists the line goes in before its first auth
line (on Debian and Ubuntu, before *@include common-auth*). Where only the
distribution's copy in _/usr/lib/pam.d_ exists, a small _/etc/pam.d/<service>_
is written that puts the line first and includes the distribution's file for
everything else. Turning it off takes out exactly that line, or that file.

The login screen is left alone on purpose: logging in is also what unlocks the
wallet, and a face has no password to hand it.

# CAMERAS

Any V4L2 camera. *automatic* picks the first colour camera. An infrared
camera (the kind Windows Hello uses) can be picked under *Settings*; its
emitter has to be switched on with a tool such as linux-enable-ir-emitter.
In infrared a phone screen shows up black, which makes the photo check's job
easier. A face set up with one camera should be set up again for another.

A laptop with its lid shut is not scanned (*Not while the lid is closed*).

# FILES

_~/.config/plasma-face-unlock/config_
	This user's settings: the lock screen, the bubble. Written by the menu.

_/etc/plasma-face-unlock/config_
	The system settings: camera, photo check, strictness, attention, scan
	length. Written by the menu through sudo.

_/var/lib/plasma-face-unlock/users/<uid>.json_
	The face data: numbers, no pictures. Root only.

_/var/lib/plasma-face-unlock/users/<uid>.state_
	Failed scans in a row, the pause they lead to, the last unlock.

_/usr/share/plasma-face-unlock/models/_
	The two networks.

_/run/plasma-face-unlock/socket_
	The daemon's socket.

_$XDG_RUNTIME_DIR/plasma-face-unlock/agent.socket_
	Where the daemon tells the agent about scans it did not start, for the
	bubble.

# ENVIRONMENT

*NO_COLOR*
	Disables colour.

# REQUIREMENTS

KDE Plasma 6 on Wayland, a camera, OpenCV 4.5.4 or newer with its DNN module,
Qt 6, LayerShellQt, KIdleTime, KI18n, systemd, polkit and Linux-PAM.

# SEE ALSO

*loginctl*(1), *pam*(8), *polkit*(8), *systemctl*(1)

# AUTHORS

Felitendo. Source and issue tracker at
https://github.com/LoonixTools/plasma-face-unlock

The liveness model and the look of the bubble follow Glance by Jonathan Zhou
(https://github.com/jonnyoo/glance, MIT). The models are YuNet and SFace from
the OpenCV model zoo (MIT and Apache-2.0).
