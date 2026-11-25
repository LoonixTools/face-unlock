//! The picture behind face-unlock's own lock screen. By default the one on
//! the desktop. Hyprland, niri and the like leave the wallpaper to a program
//! of its own, so it comes from whichever of the common ones runs: swaybg,
//! awww (once swww), hyprpaper or wpaperd, each for every output.

use std::collections::HashMap;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// A picture, or one at random from a folder.
fn picture(setting: &str) -> Option<PathBuf> {
    if setting.is_empty() {
        return None;
    }
    let path = crate::paths::untilde(setting);
    if path.is_file() {
        return Some(path);
    }
    let mut pictures: Vec<PathBuf> = std::fs::read_dir(&path)
        .ok()?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.is_file() && p.extension().and_then(|e| e.to_str()).is_some_and(|e| matches!(e.to_lowercase().as_str(), "jpg" | "jpeg" | "png" | "webp")))
        .collect();
    if pictures.is_empty() {
        return None;
    }
    pictures.sort();
    let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.subsec_nanos()).unwrap_or(0) as usize;
    Some(pictures.swap_remove(nanos % pictures.len()))
}

/// What a command prints, or nothing when it is not there or hangs.
fn run(program: &str, args: &[&str]) -> String {
    let Ok(mut child) = Command::new(program).args(args).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::null()).spawn() else {
        return String::new();
    };
    let start = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                if !status.success() {
                    return String::new();
                }
                let mut out = String::new();
                if let Some(mut s) = child.stdout.take() {
                    let _ = std::io::Read::read_to_string(&mut s, &mut out);
                }
                return out;
            }
            Ok(None) if start.elapsed() < Duration::from_secs(1) => std::thread::sleep(Duration::from_millis(10)),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return String::new();
            }
        }
    }
}

/// swaybg's arguments: "-o OUTPUT -i IMAGE" pairs, "*" for every output.
pub fn from_swaybg(args: &[String]) -> HashMap<String, String> {
    let mut found = HashMap::new();
    let mut output = String::new();
    let mut i = 0;
    while i < args.len() {
        let a = args[i].as_str();
        let next = args.get(i + 1);
        if (a == "-o" || a == "--output") && next.is_some() {
            output = next.cloned().unwrap_or_default();
            i += 1;
        } else if let Some(o) = a.strip_prefix("--output=") {
            output = o.into();
        } else if (a == "-i" || a == "--image") && next.is_some() {
            found.insert(if output == "*" { String::new() } else { output.clone() }, next.cloned().unwrap_or_default());
            i += 1;
        } else if let Some(img) = a.strip_prefix("--image=") {
            found.insert(if output == "*" { String::new() } else { output.clone() }, img.into());
        }
        i += 1;
    }
    found
}

/// `awww query` and `swww query`: "eDP-1: 1920x1080, scale: 1, currently
/// displaying: image: /a/b.jpg", with a namespace in front on awww. A plain
/// colour is no picture.
pub fn from_awww(text: &str) -> HashMap<String, String> {
    let mut found = HashMap::new();
    for line in text.lines() {
        let Some((head, image)) = line.split_once("currently displaying: image: ") else { continue };
        // The output is the last word before ": <w>x<h>,".
        let size = |s: &str| s.split(',').next().and_then(|s| s.split_once('x')).is_some_and(|(w, h)| w.trim().parse::<u32>().is_ok() && h.trim().parse::<u32>().is_ok());
        let Some(colon) = head.match_indices(": ").map(|(i, _)| i).find(|&i| size(&head[i + 2..])) else { continue };
        let output = head[..colon].split_whitespace().last().unwrap_or("").trim_end_matches(':');
        if !output.is_empty() {
            found.insert(output.to_string(), image.trim().to_string());
        }
    }
    found
}

/// `hyprctl hyprpaper listactive` and `wpaperctl all-wallpapers`: "eDP-1:
/// /a/b.jpg", on older hyprpaper "eDP-1 = /a/b.jpg".
pub fn from_list(text: &str) -> HashMap<String, String> {
    let mut found = HashMap::new();
    for line in text.lines() {
        let split = line.split_once(": /").map(|(o, p)| (o, format!("/{p}"))).or_else(|| line.split_once(" = /").map(|(o, p)| (o, format!("/{p}"))));
        if let Some((output, path)) = split
            && !output.is_empty()
            && !output.contains([' ', ':', '='])
        {
            found.insert(output.to_string(), path.trim().to_string());
        }
    }
    found
}

/// The pictures the running wallpaper programs of this user show.
fn desktop() -> HashMap<String, String> {
    let mut found = HashMap::new();
    let me = unsafe { libc::getuid() };
    let Ok(proc) = std::fs::read_dir("/proc") else { return found };
    for e in proc.filter_map(|e| e.ok()) {
        let dir = e.path();
        if !e.file_name().to_string_lossy().bytes().all(|b| b.is_ascii_digit()) || std::fs::metadata(&dir).map(|m| m.uid()).ok() != Some(me) {
            continue;
        }
        let name = std::fs::read_to_string(dir.join("comm")).unwrap_or_default();
        match name.trim() {
            "swaybg" => {
                let cmdline = std::fs::read(dir.join("cmdline")).unwrap_or_default();
                let args: Vec<String> = cmdline.split(|b| *b == 0).skip(1).map(|a| String::from_utf8_lossy(a).into_owned()).collect();
                // Relative to where it was started.
                let cwd = std::fs::read_link(dir.join("cwd")).unwrap_or_default();
                for (output, path) in from_swaybg(&args) {
                    found.insert(output, cwd.join(path).to_string_lossy().into_owned());
                }
            }
            n @ ("awww-daemon" | "swww-daemon") => found.extend(from_awww(&run(&n[..4], &["query"]))),
            "hyprpaper" => found.extend(from_list(&run("hyprctl", &["hyprpaper", "listactive"]))),
            "wpaperd" => found.extend(from_list(&run("wpaperctl", &["all-wallpapers"]))),
            _ => {}
        }
    }
    found
}

/// The pictures for a lock by output name, "" for every other output.
/// `setting` is LockWallpaper: empty for the desktop's, "none", a picture,
/// or a folder to take one from at random.
pub fn for_lock(setting: &str) -> HashMap<String, PathBuf> {
    let mut pictures = HashMap::new();
    if setting.eq_ignore_ascii_case("none") {
        return pictures;
    }
    if !setting.is_empty() {
        if let Some(p) = picture(setting) {
            pictures.insert(String::new(), p);
        }
        return pictures;
    }
    for (output, path) in desktop() {
        if let Some(p) = picture(&path) {
            pictures.insert(output, p);
        }
    }
    pictures
}

/// The one for this output.
pub fn pick<'a>(pictures: &'a HashMap<String, PathBuf>, output: &str) -> Option<&'a Path> {
    if let Some(p) = pictures.get(output).or_else(|| pictures.get("")) {
        return Some(p);
    }
    // Named differently than here: some picture of the desktop is still
    // better than none.
    let mut outputs: Vec<&String> = pictures.keys().collect();
    outputs.sort();
    outputs.first().and_then(|o| pictures.get(*o)).map(PathBuf::as_path)
}

/// The picture on the desktop, for showing something like it: the one a
/// wallpaper program shows, else Plasma's or GNOME's, else the one the
/// distribution comes with.
pub fn of_desktop() -> Option<PathBuf> {
    let programs = for_lock("");
    if let Some(p) = pick(&programs, "") {
        return Some(p.to_path_buf());
    }
    plasma().or_else(gnome).filter(|p| drawable(p)).or_else(of_distro)
}

/// The sections of a KDE settings file, by their name ("[A][B]" becomes
/// "A][B"), each with its keys.
fn kde_sections(text: &str) -> Vec<(String, HashMap<String, String>)> {
    let mut out: Vec<(String, HashMap<String, String>)> = Vec::new();
    for line in text.lines() {
        if let Some(name) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
            out.push((name.to_string(), HashMap::new()));
        } else if let (Some((k, v)), Some(last)) = (line.split_once('='), out.last_mut()) {
            last.1.insert(k.trim().to_string(), v.trim().to_string());
        }
    }
    out
}

/// Plasma keeps it with the desktop of each screen and activity: a file, a
/// folder for a slideshow, or a wallpaper package with pictures in several
/// sizes. A desktop that names none shows Plasma's default package.
fn plasma() -> Option<PathBuf> {
    let text = std::fs::read_to_string(crate::paths::untilde("~/.config/plasma-org.kde.plasma.desktop-appletsrc")).ok()?;
    let sections = kde_sections(&text);
    let get = |name: &str| sections.iter().find(|(n, _)| n == name).map(|(_, keys)| keys);
    let activity = std::fs::read_to_string(crate::paths::untilde("~/.config/kactivitymanagerdrc"))
        .ok()
        .and_then(|t| kde_sections(&t).into_iter().find(|(n, _)| n == "main").and_then(|(_, k)| k.get("currentActivity").cloned()));
    // The desktops: not panels. This activity's first, then the first screen's.
    let mut desktops: Vec<(&String, &HashMap<String, String>)> = sections
        .iter()
        .filter(|(n, keys)| n.starts_with("Containments][") && n.matches("][").count() == 1 && keys.get("formfactor").is_none_or(|f| f == "0") && keys.contains_key("wallpaperplugin"))
        .map(|(n, k)| (n, k))
        .collect();
    desktops.sort_by_key(|(_, k)| (activity.as_ref().is_some_and(|a| k.get("activityId") != Some(a)), k.get("lastScreen").map(|s| s != "0").unwrap_or(true)));
    let (name, keys) = desktops.first()?;
    let screen = get(&format!("{name}][General")).and_then(|g| g.get("lastResolution")).and_then(|r| r.split_once('x')).and_then(|(w, h)| Some((w.parse().ok()?, h.parse().ok()?)));
    match keys.get("wallpaperplugin").map(String::as_str) {
        Some("org.kde.slideshow") => {
            let paths = get(&format!("{name}][Wallpaper][org.kde.slideshow][General"))?.get("SlidePaths")?;
            paths.split(',').find_map(|p| picture(p.trim().trim_start_matches("file://")))
        }
        Some("org.kde.image") | None => {
            let image = get(&format!("{name}][Wallpaper][org.kde.image][General")).and_then(|g| g.get("Image")).filter(|i| !i.is_empty()).cloned();
            let path = match image {
                Some(i) => PathBuf::from(i.trim_start_matches("file://")),
                None => default_plasma_wallpaper()?,
            };
            if path.is_file() { Some(path) } else { from_package(&path, screen) }
        }
        _ => None,
    }
}

/// The wallpaper package Plasma shows when none is set: "Next" unless the
/// system says otherwise.
fn default_plasma_wallpaper() -> Option<PathBuf> {
    let theme = ["~/.config/plasmarc", "/etc/xdg/plasmarc"]
        .iter()
        .filter_map(|f| std::fs::read_to_string(crate::paths::untilde(f)).ok())
        .find_map(|t| kde_sections(&t).into_iter().find(|(n, _)| n == "Wallpapers").and_then(|(_, k)| k.get("defaultWallpaperTheme").cloned()))
        .unwrap_or_else(|| "Next".into());
    ["~/.local/share/wallpapers", "/usr/share/wallpapers"].iter().map(|d| crate::paths::untilde(d).join(&theme)).find(|p| p.is_dir())
}

/// Whether Plasma runs with a dark colour scheme, as a wallpaper package then
/// shows its dark pictures.
fn plasma_dark() -> bool {
    let text = std::fs::read_to_string(crate::paths::untilde("~/.config/kdeglobals")).unwrap_or_default();
    let sections = kde_sections(&text);
    let key = |section: &str, key: &str| sections.iter().find(|(n, _)| n == section).and_then(|(_, k)| k.get(key)).map(|v| v.to_lowercase());
    key("General", "ColorScheme").or_else(|| key("KDE", "LookAndFeelPackage")).is_some_and(|v| v.contains("dark"))
}

/// The picture of a wallpaper package for a screen of this size: its
/// pictures are named after their size ("5120x2880.png"). The one closest
/// in shape, and of those the biggest.
fn from_package(dir: &Path, screen: Option<(u32, u32)>) -> Option<PathBuf> {
    let dark = dir.join("contents/images_dark");
    let images = if plasma_dark() && dark.is_dir() { dark } else { dir.join("contents/images") };
    let (sw, sh) = screen.unwrap_or((16, 9));
    let want = sw as f64 / sh.max(1) as f64;
    let size = |p: &Path| -> Option<(u32, u32)> {
        let stem = p.file_stem()?.to_str()?;
        let (w, h) = stem.split_once('x')?;
        Some((w.parse().ok()?, h.parse().ok()?))
    };
    let mut pictures: Vec<(PathBuf, (u32, u32))> =
        std::fs::read_dir(&images).ok()?.filter_map(|e| e.ok().map(|e| e.path())).filter(|p| drawable(p)).filter_map(|p| size(&p).map(|s| (p, s))).collect();
    pictures.sort_by(|(_, a), (_, b)| {
        let off = |(w, h): (u32, u32)| ((w as f64 / h.max(1) as f64) - want).abs();
        off(*a).total_cmp(&off(*b)).then(b.0.cmp(&a.0))
    });
    pictures.into_iter().next().map(|(p, _)| p).or_else(|| biggest(&images, 1)).or_else(|| biggest(dir, 3))
}

fn gnome() -> Option<PathBuf> {
    let dark = run("gsettings", &["get", "org.gnome.desktop.interface", "color-scheme"]).contains("dark");
    let uri = run("gsettings", &["get", "org.gnome.desktop.background", if dark { "picture-uri-dark" } else { "picture-uri" }]);
    let path = PathBuf::from(uri.trim().trim_matches('\'').trim_start_matches("file://"));
    path.is_file().then_some(path)
}

/// A well-known distribution: where it puts its wallpapers, and its colour.
struct Distro {
    id: &'static str,
    /// Folders or files to look in, first to last. A last part ending in
    /// `*` is every entry starting with what comes before it, newest first.
    wallpapers: &'static [&'static str],
    color: Option<[u8; 3]>,
}

const DISTROS: &[Distro] = &[
    Distro { id: "cachyos", wallpapers: &["/usr/share/wallpapers/cachyos-wallpapers"], color: None },
    Distro { id: "arch", wallpapers: &["/usr/share/backgrounds/archlinux"], color: Some([0x17, 0x93, 0xd1]) },
    Distro { id: "endeavouros", wallpapers: &["/usr/share/endeavouros/backgrounds"], color: Some([0x7f, 0x3f, 0xbf]) },
    Distro { id: "manjaro", wallpapers: &["/usr/share/backgrounds/manjaro", "/usr/share/wallpapers/manjaro*"], color: Some([0x35, 0xbf, 0x5c]) },
    Distro { id: "fedora", wallpapers: &["/usr/share/wallpapers/Fedora", "/usr/share/backgrounds/f*"], color: Some([0x3c, 0x6e, 0xb4]) },
    Distro { id: "ubuntu", wallpapers: &["/usr/share/backgrounds/warty-final-ubuntu.png", "/usr/share/backgrounds"], color: Some([0xe9, 0x54, 0x20]) },
    Distro { id: "debian", wallpapers: &["/usr/share/desktop-base/active-theme/wallpaper", "/usr/share/images/desktop-base"], color: Some([0xa8, 0x00, 0x30]) },
    Distro { id: "linuxmint", wallpapers: &["/usr/share/backgrounds/linuxmint/default_background.jpg", "/usr/share/backgrounds/linuxmint"], color: Some([0x86, 0xbe, 0x43]) },
    Distro { id: "opensuse", wallpapers: &["/usr/share/wallpapers/openSUSEdefault", "/usr/share/wallpapers/default*"], color: Some([0x73, 0xba, 0x25]) },
    Distro { id: "pop", wallpapers: &["/usr/share/backgrounds/pop"], color: Some([0x48, 0xb9, 0xc7]) },
    Distro { id: "zorin", wallpapers: &["/usr/share/backgrounds/zorin*"], color: Some([0x15, 0xa6, 0xf0]) },
    Distro { id: "elementary", wallpapers: &["/usr/share/backgrounds/elementaryos-default", "/usr/share/backgrounds"], color: Some([0x36, 0x89, 0xe6]) },
];

/// The names os-release gives this system: its own, then the ones it is
/// like ("opensuse-tumbleweed", "opensuse", "suse").
fn os_names() -> Vec<String> {
    ["ID", "ID_LIKE"].iter().filter_map(|k| crate::system::os_release(k)).flat_map(|v| v.split_whitespace().map(str::to_lowercase).collect::<Vec<_>>()).collect()
}

/// The well-known distribution this is, or is based on.
fn distro() -> Option<&'static Distro> {
    let names = os_names();
    names.iter().find_map(|n| DISTROS.iter().find(|d| d.id == n || n.starts_with(&format!("{}-", d.id))))
}

/// The colour of the distribution, for drawing something that looks like
/// it: from the list above, else from os-release when it is a 24-bit one
/// ("38;2;23;147;209").
pub fn distro_color() -> Option<[u8; 3]> {
    if let Some(c) = distro().and_then(|d| d.color) {
        return Some(c);
    }
    let code = crate::system::os_release("ANSI_COLOR")?;
    let parts: Vec<u8> = code.split(';').filter_map(|p| p.trim().parse().ok()).collect();
    let i = parts.windows(2).position(|w| w == [38, 2])?;
    Some([*parts.get(i + 2)?, *parts.get(i + 3)?, *parts.get(i + 4)?])
}

/// `path`, or with a last part ending in `*`, what it matches: the highest
/// number first (f41 before f40).
fn expand(path: &str) -> Vec<PathBuf> {
    let Some(prefix) = path.strip_suffix('*') else { return vec![PathBuf::from(path)] };
    let (dir, start) = prefix.rsplit_once('/').unwrap_or(("", prefix));
    let Ok(entries) = std::fs::read_dir(dir) else { return vec![] };
    let mut found: Vec<PathBuf> = entries.filter_map(|e| e.ok().map(|e| e.path())).filter(|p| p.file_name().is_some_and(|n| n.to_string_lossy().starts_with(start))).collect();
    let number = |p: &PathBuf| p.file_name().map(|n| n.to_string_lossy().chars().filter(char::is_ascii_digit).collect::<String>().parse::<u64>().unwrap_or(0)).unwrap_or(0);
    found.sort_by_key(|p| std::cmp::Reverse(number(p)));
    found
}

/// The wallpaper the distribution installs: from the list of well-known
/// ones, else found by its name in the usual folders. Else Plasma's or
/// GNOME's own.
fn of_distro() -> Option<PathBuf> {
    if let Some(d) = distro() {
        for path in d.wallpapers.iter().flat_map(|p| expand(p)) {
            let found = if path.join("contents/images").is_dir() { from_package(&path, None) } else { biggest(&path, 3) };
            if found.is_some() {
                return found;
            }
        }
    }
    let mut names: Vec<String> = os_names().into_iter().filter(|n| n.len() >= 3).collect();
    names.extend(["next".to_string(), "gnome".to_string()]);
    for name in &names {
        for root in ["/usr/share/wallpapers", "/usr/share/backgrounds"] {
            let Ok(entries) = std::fs::read_dir(root) else { continue };
            let mut hits: Vec<PathBuf> = entries
                .filter_map(|e| e.ok().map(|e| e.path()))
                .filter(|p| p.file_name().is_some_and(|n| n.to_string_lossy().to_lowercase().contains(name.as_str())))
                .collect();
            hits.sort();
            if let Some(p) = hits.iter().find_map(|h| if h.join("contents/images").is_dir() { from_package(h, None) } else { biggest(h, 3) }) {
                return Some(p);
            }
        }
    }
    None
}

/// A picture the window can draw.
fn drawable(path: &Path) -> bool {
    path.extension().and_then(|e| e.to_str()).is_some_and(|e| matches!(e.to_lowercase().as_str(), "jpg" | "jpeg" | "png" | "webp"))
}

/// The picture at `path`, or the biggest one in it and its folders: the one
/// in the highest resolution.
fn biggest(path: &Path, depth: u32) -> Option<PathBuf> {
    if path.is_file() {
        return drawable(path).then(|| path.to_path_buf());
    }
    if depth == 0 {
        return None;
    }
    let mut best: Option<(u64, PathBuf)> = None;
    for e in std::fs::read_dir(path).ok()?.filter_map(|e| e.ok()) {
        let Some(found) = biggest(&e.path(), depth - 1) else { continue };
        let size = found.metadata().map(|m| m.len()).unwrap_or(0);
        if best.as_ref().is_none_or(|(s, _)| size > *s) {
            best = Some((size, found));
        }
    }
    best.map(|(_, p)| p)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kde_sections_by_name() {
        let text = "[Containments][43]\nplugin=org.kde.plasma.folder\nwallpaperplugin=org.kde.image\n\n[Containments][43][Wallpaper][org.kde.image][General]\nImage=/a.png\n";
        let s = kde_sections(text);
        assert_eq!(s.len(), 2);
        assert_eq!(s[0].0, "Containments][43");
        assert_eq!(s[1].0, "Containments][43][Wallpaper][org.kde.image][General");
        assert_eq!(s[1].1.get("Image").map(String::as_str), Some("/a.png"));
    }

    #[test]
    fn newest_folder_first() {
        let dir = std::env::temp_dir().join(format!("fu-expand-{}", std::process::id()));
        for d in ["f39", "f41", "f40", "other"] {
            std::fs::create_dir_all(dir.join(d)).unwrap();
        }
        let found = expand(&format!("{}/f*", dir.display()));
        assert_eq!(found, vec![dir.join("f41"), dir.join("f40"), dir.join("f39")]);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn biggest_picture_in_a_package() {
        let dir = std::env::temp_dir().join(format!("fu-wallpaper-{}", std::process::id()));
        let images = dir.join("Next/contents/images");
        std::fs::create_dir_all(&images).unwrap();
        std::fs::write(images.join("1280x720.png"), [0u8; 10]).unwrap();
        std::fs::write(images.join("3840x2160.png"), [0u8; 100]).unwrap();
        std::fs::write(images.join("notes.txt"), [0u8; 1000]).unwrap();
        assert_eq!(biggest(&dir.join("Next"), 3), Some(images.join("3840x2160.png")));
        assert_eq!(biggest(&dir.join("Next"), 1), None);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn swaybg() {
        let a: Vec<String> = ["-o", "*", "-i", "/all.png", "--output=DP-1", "--image=/dp.png"].iter().map(|s| s.to_string()).collect();
        let f = from_swaybg(&a);
        assert_eq!(f.get(""), Some(&"/all.png".to_string()));
        assert_eq!(f.get("DP-1"), Some(&"/dp.png".to_string()));
    }

    #[test]
    fn awww_and_lists() {
        let f = from_awww("ns: eDP-1: 1920x1080, scale: 1, currently displaying: image: /a/b c.jpg\nDP-2: 2560x1440, scale: 1, currently displaying: color: 000000\n");
        assert_eq!(f.get("eDP-1"), Some(&"/a/b c.jpg".to_string()));
        assert!(!f.contains_key("DP-2"));
        let l = from_list("eDP-1: /a/b.jpg\nDP-1 = /c/d.png\nnonsense\n");
        assert_eq!(l.get("eDP-1"), Some(&"/a/b.jpg".to_string()));
        assert_eq!(l.get("DP-1"), Some(&"/c/d.png".to_string()));
        assert_eq!(l.len(), 2);
    }

    #[test]
    fn picks_by_output_then_any() {
        let mut p = HashMap::new();
        p.insert("DP-1".to_string(), PathBuf::from("/a"));
        assert_eq!(pick(&p, "DP-1"), Some(Path::new("/a")));
        assert_eq!(pick(&p, "HDMI-1"), Some(Path::new("/a")));
        p.insert(String::new(), PathBuf::from("/all"));
        assert_eq!(pick(&p, "HDMI-1"), Some(Path::new("/all")));
    }
}
