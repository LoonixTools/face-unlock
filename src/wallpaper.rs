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

#[cfg(test)]
mod tests {
    use super::*;

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
