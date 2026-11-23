//! Where frames come from.
//!
//! Normally a V4L2 device, read straight through the kernel's interface. For
//! testing without a camera (a virtual machine, a build server) a video file
//! or a folder of pictures stands in for one and is played back at the pace
//! a camera would deliver it, so anything timed in the liveness checks
//! behaves the way it would with the real thing.

use crate::img::Bgr;
use std::ffi::CString;
use std::io::Read;
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

const WIDTH: u32 = 640;
const HEIGHT: u32 = 480;
// Pictures stand in for a camera at this rate, each held for a while so the
// checks see a still face the way they would see a person holding still.
const IMAGE_FRAME: Duration = Duration::from_micros(1_000_000 / 15);
const IMAGE_REPEAT: u32 = 12;

#[derive(Clone, Debug, PartialEq)]
pub struct Info {
    pub path: String,
    pub name: String,
    /// Infrared cameras (the kind Windows Hello uses) only offer grey formats.
    pub infrared: bool,
}

// ---------------------------------------------------------------------------
// The kernel's interface
// ---------------------------------------------------------------------------

mod v4l2 {
    use libc::{c_ulong, timeval};

    pub const CAP_VIDEO_CAPTURE: u32 = 0x0000_0001;
    pub const CAP_META_CAPTURE: u32 = 0x0080_0000;
    pub const CAP_DEVICE_CAPS: u32 = 0x8000_0000;
    pub const BUF_TYPE_VIDEO_CAPTURE: u32 = 1;
    pub const MEMORY_MMAP: u32 = 1;
    pub const FIELD_ANY: u32 = 0;

    pub const fn fourcc(s: &[u8; 4]) -> u32 {
        s[0] as u32 | (s[1] as u32) << 8 | (s[2] as u32) << 16 | (s[3] as u32) << 24
    }
    pub const MJPG: u32 = fourcc(b"MJPG");
    pub const JPEG: u32 = fourcc(b"JPEG");
    pub const YUYV: u32 = fourcc(b"YUYV");
    pub const GREY: u32 = fourcc(b"GREY");
    pub const Y10: u32 = fourcc(b"Y10 ");
    pub const Y12: u32 = fourcc(b"Y12 ");
    pub const Y16: u32 = fourcc(b"Y16 ");

    #[repr(C)]
    pub struct Capability {
        pub driver: [u8; 16],
        pub card: [u8; 32],
        pub bus_info: [u8; 32],
        pub version: u32,
        pub capabilities: u32,
        pub device_caps: u32,
        pub reserved: [u32; 3],
    }

    #[repr(C)]
    pub struct FmtDesc {
        pub index: u32,
        pub type_: u32,
        pub flags: u32,
        pub description: [u8; 32],
        pub pixelformat: u32,
        pub mbus_code: u32,
        pub reserved: [u32; 3],
    }

    #[repr(C)]
    #[derive(Clone, Copy)]
    pub struct PixFormat {
        pub width: u32,
        pub height: u32,
        pub pixelformat: u32,
        pub field: u32,
        pub bytesperline: u32,
        pub sizeimage: u32,
        pub colorspace: u32,
        pub priv_: u32,
        pub flags: u32,
        pub enc: u32,
        pub quantization: u32,
        pub xfer_func: u32,
    }

    /// The union is 200 bytes, aligned like a pointer.
    #[repr(C)]
    pub union FormatUnion {
        pub pix: PixFormat,
        pub raw: [c_ulong; 200 / std::mem::size_of::<c_ulong>()],
    }

    #[repr(C)]
    pub struct Format {
        pub type_: u32,
        pub fmt: FormatUnion,
    }

    #[repr(C)]
    pub struct RequestBuffers {
        pub count: u32,
        pub type_: u32,
        pub memory: u32,
        pub capabilities: u32,
        pub flags: u8,
        pub reserved: [u8; 3],
    }

    #[repr(C)]
    pub struct Timecode {
        pub type_: u32,
        pub flags: u32,
        pub frames: u8,
        pub seconds: u8,
        pub minutes: u8,
        pub hours: u8,
        pub userbits: [u8; 4],
    }

    #[repr(C)]
    pub struct Buffer {
        pub index: u32,
        pub type_: u32,
        pub bytesused: u32,
        pub flags: u32,
        pub field: u32,
        pub timestamp: timeval,
        pub timecode: Timecode,
        pub sequence: u32,
        pub memory: u32,
        /// The offset for MMAP, in a union as wide as a pointer.
        pub m: c_ulong,
        pub length: u32,
        pub reserved2: u32,
        pub request_fd: i32,
    }

    #[repr(C)]
    pub struct Fract {
        pub numerator: u32,
        pub denominator: u32,
    }

    #[repr(C)]
    pub struct CaptureParm {
        pub capability: u32,
        pub capturemode: u32,
        pub timeperframe: Fract,
        pub extendedmode: u32,
        pub readbuffers: u32,
        pub reserved: [u32; 4],
    }

    #[repr(C)]
    pub struct StreamParm {
        pub type_: u32,
        pub parm: CaptureParm,
        pub pad: [u8; 200 - std::mem::size_of::<CaptureParm>()],
    }

    const fn ioc(dir: u64, nr: u64, size: usize) -> u64 {
        (dir << 30) | ((size as u64) << 16) | ((b'V' as u64) << 8) | nr
    }
    const R: u64 = 2;
    const W: u64 = 1;
    const RW: u64 = 3;
    use std::mem::size_of;
    pub const QUERYCAP: u64 = ioc(R, 0, size_of::<Capability>());
    pub const ENUM_FMT: u64 = ioc(RW, 2, size_of::<FmtDesc>());
    pub const S_FMT: u64 = ioc(RW, 5, size_of::<Format>());
    pub const REQBUFS: u64 = ioc(RW, 8, size_of::<RequestBuffers>());
    pub const QUERYBUF: u64 = ioc(RW, 9, size_of::<Buffer>());
    pub const QBUF: u64 = ioc(RW, 15, size_of::<Buffer>());
    pub const DQBUF: u64 = ioc(RW, 17, size_of::<Buffer>());
    pub const STREAMON: u64 = ioc(W, 18, size_of::<i32>());
    pub const STREAMOFF: u64 = ioc(W, 19, size_of::<i32>());
    pub const S_PARM: u64 = ioc(RW, 22, size_of::<StreamParm>());

    #[cfg(target_pointer_width = "64")]
    const _: () = {
        assert!(size_of::<Capability>() == 104);
        assert!(size_of::<Format>() == 208);
        assert!(size_of::<Buffer>() == 88);
        assert!(size_of::<StreamParm>() == 204);
        assert!(QUERYCAP == 0x8068_5600);
        assert!(S_FMT == 0xc0d0_5605);
    };
}

fn ioctl<T>(fd: i32, req: u64, arg: &mut T) -> std::io::Result<()> {
    loop {
        // SAFETY: arg points to a struct of the size encoded in req.
        let r = unsafe { libc::ioctl(fd, req as _, arg as *mut T) };
        if r == 0 {
            return Ok(());
        }
        let e = std::io::Error::last_os_error();
        if e.raw_os_error() != Some(libc::EINTR) {
            return Err(e);
        }
    }
}

fn open_raw(path: &str, flags: i32) -> std::io::Result<OwnedFd> {
    let c = CString::new(path).map_err(|_| std::io::Error::from(std::io::ErrorKind::InvalidInput))?;
    // SAFETY: a valid C string.
    let fd = unsafe { libc::open(c.as_ptr(), flags | libc::O_CLOEXEC) };
    if fd < 0 {
        return Err(std::io::Error::last_os_error());
    }
    // SAFETY: we just opened it.
    Ok(unsafe { OwnedFd::from_raw_fd(fd) })
}

fn zeroed<T>() -> T {
    // SAFETY: only used on plain kernel structs, for which zero is valid.
    unsafe { std::mem::zeroed() }
}

fn cstr(b: &[u8]) -> String {
    let end = b.iter().position(|&c| c == 0).unwrap_or(b.len());
    String::from_utf8_lossy(&b[..end]).trim().to_string()
}

// UVC names a node "<device>: <function>", often twice the same or with the
// second cut short. The function tells a laptop's two cameras apart ("HP HD
// Camera: HP IR Camera"), so that part is kept unless it only repeats the
// first.
fn short_name(name: &str) -> String {
    match name.split_once(": ") {
        Some((device, function)) => {
            let function = function.trim();
            if function.is_empty() || device.starts_with(function) { device.into() } else { function.into() }
        }
        None => name.into(),
    }
}

/// Opens the node just long enough to ask what it is. Neither this nor the
/// format list starts streaming, so it does not switch the light on.
fn probe(path: &str) -> Option<Info> {
    let fd = open_raw(path, libc::O_RDONLY | libc::O_NONBLOCK).ok()?;
    let mut cap: v4l2::Capability = zeroed();
    ioctl(fd.as_raw_fd(), v4l2::QUERYCAP, &mut cap).ok()?;
    let caps = if cap.capabilities & v4l2::CAP_DEVICE_CAPS != 0 { cap.device_caps } else { cap.capabilities };
    if caps & v4l2::CAP_VIDEO_CAPTURE == 0 || caps & v4l2::CAP_META_CAPTURE != 0 {
        return None;
    }
    let (mut colour, mut grey) = (false, false);
    for i in 0.. {
        let mut f: v4l2::FmtDesc = zeroed();
        f.index = i;
        f.type_ = v4l2::BUF_TYPE_VIDEO_CAPTURE;
        if ioctl(fd.as_raw_fd(), v4l2::ENUM_FMT, &mut f).is_err() {
            break;
        }
        match f.pixelformat {
            v4l2::GREY | v4l2::Y10 | v4l2::Y12 | v4l2::Y16 => grey = true,
            _ => colour = true,
        }
    }
    if !colour && !grey {
        return None;
    }
    let node = Path::new(path).file_name()?.to_string_lossy().to_string();
    let sys = std::fs::read_to_string(format!("/sys/class/video4linux/{node}/name")).map(|s| s.trim().to_string()).unwrap_or_default();
    let name = if sys.is_empty() { cstr(&cap.card) } else { sys };
    Some(Info { path: path.into(), name: short_name(&name), infrared: grey && !colour })
}

/// A program that streams holds the camera's buffers. Asking for none is how
/// V4L2 lets a program check without taking them: the driver says EBUSY when
/// they belong to somebody else.
fn busy(path: &str) -> bool {
    let fd = match open_raw(path, libc::O_RDWR | libc::O_NONBLOCK) {
        Ok(fd) => fd,
        Err(e) => return e.raw_os_error() == Some(libc::EBUSY),
    };
    let mut req: v4l2::RequestBuffers = zeroed();
    req.type_ = v4l2::BUF_TYPE_VIDEO_CAPTURE;
    req.memory = v4l2::MEMORY_MMAP;
    matches!(ioctl(fd.as_raw_fd(), v4l2::REQBUFS, &mut req), Err(e) if e.raw_os_error() == Some(libc::EBUSY))
}

/// Every V4L2 device that can capture video. Metadata nodes, which every UVC
/// camera also exposes, are left out.
pub fn list() -> Vec<Info> {
    let mut nodes: Vec<u32> = std::fs::read_dir("/dev")
        .map(|d| d.filter_map(|e| e.ok()?.file_name().to_str()?.strip_prefix("video")?.parse().ok()).collect())
        .unwrap_or_default();
    nodes.sort();
    nodes.into_iter().filter_map(|n| probe(&format!("/dev/video{n}"))).collect()
}

/// What "auto" means on this machine: the first colour camera, else the
/// first camera at all.
pub fn auto_path() -> String {
    let all = list();
    all.iter().find(|c| !c.infrared).or(all.first()).map(|c| c.path.clone()).unwrap_or_default()
}

/// Whether another program streams from the camera right now, a video call
/// for example. Asked without starting it, so the light stays off. Never
/// true for a video file or pictures.
pub fn in_use(spec: &str) -> bool {
    if spec.starts_with("file:") || spec.starts_with("images:") {
        return false;
    }
    let path = if spec.is_empty() || spec == "auto" { auto_path() } else { spec.into() };
    !path.is_empty() && busy(&path)
}

// ---------------------------------------------------------------------------
// The camera
// ---------------------------------------------------------------------------

enum Source {
    Device(Device),
    Images { pictures: Vec<Bgr>, index: usize, repeat: u32 },
    Video { child: std::process::Child, w: usize, h: usize, frame: Duration },
}

pub struct Camera {
    source: Source,
    start: Instant,
    next: Instant,
    pub infrared: bool,
    pub description: String,
}

struct Device {
    fd: OwnedFd,
    buffers: Vec<(*mut libc::c_void, usize)>,
    format: u32,
    w: usize,
    h: usize,
}

// The mappings belong to this one camera and are only touched by whoever
// owns it.
unsafe impl Send for Device {}

impl Drop for Device {
    fn drop(&mut self) {
        let mut t = v4l2::BUF_TYPE_VIDEO_CAPTURE as i32;
        let _ = ioctl(self.fd.as_raw_fd(), v4l2::STREAMOFF, &mut t);
        for &(p, len) in &self.buffers {
            // SAFETY: mapped in open_device with this length.
            unsafe { libc::munmap(p, len) };
        }
    }
}

impl Device {
    fn open(path: &str, infrared: bool) -> std::io::Result<Device> {
        let fd = open_raw(path, libc::O_RDWR | libc::O_NONBLOCK)?;
        let raw = fd.as_raw_fd();
        // MJPEG gets a webcam its full frame rate over USB 2; raw YUYV at
        // 640x480 often tops out at 15 fps. Grey for an infrared camera.
        let wanted: &[u32] = if infrared { &[v4l2::GREY, v4l2::Y16, v4l2::Y10, v4l2::YUYV] } else { &[v4l2::MJPG, v4l2::YUYV, v4l2::JPEG, v4l2::GREY] };
        let mut got = None;
        for &pf in wanted {
            let mut f: v4l2::Format = zeroed();
            f.type_ = v4l2::BUF_TYPE_VIDEO_CAPTURE;
            f.fmt.pix.width = WIDTH;
            f.fmt.pix.height = HEIGHT;
            f.fmt.pix.pixelformat = pf;
            f.fmt.pix.field = v4l2::FIELD_ANY;
            if ioctl(raw, v4l2::S_FMT, &mut f).is_ok() {
                // SAFETY: the driver filled in pix.
                let pix = unsafe { f.fmt.pix };
                if pix.pixelformat == pf {
                    got = Some(pix);
                    break;
                }
            }
        }
        let pix = got.ok_or_else(|| std::io::Error::other("the camera offers no format this can read"))?;

        let mut parm: v4l2::StreamParm = zeroed();
        parm.type_ = v4l2::BUF_TYPE_VIDEO_CAPTURE;
        parm.parm.timeperframe = v4l2::Fract { numerator: 1, denominator: 30 };
        let _ = ioctl(raw, v4l2::S_PARM, &mut parm);

        // A stale frame in the driver's queue is a picture of whoever sat
        // there a moment ago. Two buffers, the fewest drivers take.
        let mut req: v4l2::RequestBuffers = zeroed();
        req.count = 2;
        req.type_ = v4l2::BUF_TYPE_VIDEO_CAPTURE;
        req.memory = v4l2::MEMORY_MMAP;
        ioctl(raw, v4l2::REQBUFS, &mut req)?;
        let mut dev = Device { fd, buffers: Vec::new(), format: pix.pixelformat, w: pix.width as usize, h: pix.height as usize };
        for i in 0..req.count {
            let mut b: v4l2::Buffer = zeroed();
            b.index = i;
            b.type_ = v4l2::BUF_TYPE_VIDEO_CAPTURE;
            b.memory = v4l2::MEMORY_MMAP;
            ioctl(raw, v4l2::QUERYBUF, &mut b)?;
            // SAFETY: the offset and length the driver gave for this buffer.
            let p = unsafe { libc::mmap(std::ptr::null_mut(), b.length as usize, libc::PROT_READ | libc::PROT_WRITE, libc::MAP_SHARED, raw, b.m as libc::off_t) };
            if p == libc::MAP_FAILED {
                return Err(std::io::Error::last_os_error());
            }
            dev.buffers.push((p, b.length as usize));
            ioctl(raw, v4l2::QBUF, &mut b)?;
        }
        let mut t = v4l2::BUF_TYPE_VIDEO_CAPTURE as i32;
        ioctl(raw, v4l2::STREAMON, &mut t)?;
        Ok(dev)
    }

    fn read(&mut self) -> Option<Bgr> {
        let raw = self.fd.as_raw_fd();
        let mut pfd = libc::pollfd { fd: raw, events: libc::POLLIN, revents: 0 };
        // SAFETY: one valid pollfd.
        if unsafe { libc::poll(&mut pfd, 1, 2000) } <= 0 {
            return None;
        }
        let mut b: v4l2::Buffer = zeroed();
        b.type_ = v4l2::BUF_TYPE_VIDEO_CAPTURE;
        b.memory = v4l2::MEMORY_MMAP;
        ioctl(raw, v4l2::DQBUF, &mut b).ok()?;
        let (p, len) = *self.buffers.get(b.index as usize)?;
        let used = (b.bytesused as usize).min(len);
        // SAFETY: the mapping is valid and the driver handed this buffer to
        // us until it is queued again.
        let data = unsafe { std::slice::from_raw_parts(p as *const u8, used) };
        let frame = decode(self.format, data, self.w, self.h);
        let _ = ioctl(raw, v4l2::QBUF, &mut b);
        frame
    }
}

fn decode(format: u32, data: &[u8], w: usize, h: usize) -> Option<Bgr> {
    match format {
        v4l2::MJPG | v4l2::JPEG => decode_jpeg(data),
        v4l2::YUYV => {
            if data.len() < w * h * 2 {
                return None;
            }
            let mut out = Bgr::new(w, h);
            for (i, q) in data[..w * h * 2].as_chunks::<4>().0.iter().enumerate() {
                let (y0, u, y1, v) = (q[0] as f32, q[1] as f32 - 128.0, q[2] as f32, q[3] as f32 - 128.0);
                for (k, y) in [y0, y1].into_iter().enumerate() {
                    let y = 1.164 * (y - 16.0);
                    let r = y + 1.596 * v;
                    let g = y - 0.813 * v - 0.391 * u;
                    let b = y + 2.018 * u;
                    let o = (i * 2 + k) * 3;
                    out.data[o] = b.clamp(0.0, 255.0) as u8;
                    out.data[o + 1] = g.clamp(0.0, 255.0) as u8;
                    out.data[o + 2] = r.clamp(0.0, 255.0) as u8;
                }
            }
            Some(out)
        }
        v4l2::GREY => {
            if data.len() < w * h {
                return None;
            }
            Some(Bgr { w, h, data: data[..w * h].iter().flat_map(|&v| [v, v, v]).collect() })
        }
        v4l2::Y10 | v4l2::Y12 | v4l2::Y16 => {
            if data.len() < w * h * 2 {
                return None;
            }
            let shift = match format {
                v4l2::Y10 => 2,
                v4l2::Y12 => 4,
                _ => 8,
            };
            Some(Bgr { w, h, data: data[..w * h * 2].as_chunks::<2>().0.iter().flat_map(|c| {
                let v = ((u16::from_le_bytes([c[0], c[1]]) >> shift).min(255)) as u8;
                [v, v, v]
            }).collect() })
        }
        _ => None,
    }
}

/// The Huffman tables every MJPEG camera leaves out, from the JPEG standard.
const DHT: &[u8] = &[
    0xff, 0xc4, 0x01, 0xa2, 0x00, 0x00, 0x01, 0x05, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x01, 0x02, 0x03, 0x04,
    0x05, 0x06, 0x07, 0x08, 0x09, 0x0a, 0x0b, 0x01, 0x00, 0x03, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x01,
    0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0a, 0x0b, 0x10, 0x00, 0x02, 0x01, 0x03, 0x03, 0x02, 0x04, 0x03, 0x05, 0x05, 0x04, 0x04, 0x00, 0x00, 0x01,
    0x7d, 0x01, 0x02, 0x03, 0x00, 0x04, 0x11, 0x05, 0x12, 0x21, 0x31, 0x41, 0x06, 0x13, 0x51, 0x61, 0x07, 0x22, 0x71, 0x14, 0x32, 0x81, 0x91, 0xa1, 0x08, 0x23,
    0x42, 0xb1, 0xc1, 0x15, 0x52, 0xd1, 0xf0, 0x24, 0x33, 0x62, 0x72, 0x82, 0x09, 0x0a, 0x16, 0x17, 0x18, 0x19, 0x1a, 0x25, 0x26, 0x27, 0x28, 0x29, 0x2a, 0x34,
    0x35, 0x36, 0x37, 0x38, 0x39, 0x3a, 0x43, 0x44, 0x45, 0x46, 0x47, 0x48, 0x49, 0x4a, 0x53, 0x54, 0x55, 0x56, 0x57, 0x58, 0x59, 0x5a, 0x63, 0x64, 0x65, 0x66,
    0x67, 0x68, 0x69, 0x6a, 0x73, 0x74, 0x75, 0x76, 0x77, 0x78, 0x79, 0x7a, 0x83, 0x84, 0x85, 0x86, 0x87, 0x88, 0x89, 0x8a, 0x92, 0x93, 0x94, 0x95, 0x96, 0x97,
    0x98, 0x99, 0x9a, 0xa2, 0xa3, 0xa4, 0xa5, 0xa6, 0xa7, 0xa8, 0xa9, 0xaa, 0xb2, 0xb3, 0xb4, 0xb5, 0xb6, 0xb7, 0xb8, 0xb9, 0xba, 0xc2, 0xc3, 0xc4, 0xc5, 0xc6,
    0xc7, 0xc8, 0xc9, 0xca, 0xd2, 0xd3, 0xd4, 0xd5, 0xd6, 0xd7, 0xd8, 0xd9, 0xda, 0xe1, 0xe2, 0xe3, 0xe4, 0xe5, 0xe6, 0xe7, 0xe8, 0xe9, 0xea, 0xf1, 0xf2, 0xf3,
    0xf4, 0xf5, 0xf6, 0xf7, 0xf8, 0xf9, 0xfa, 0x11, 0x00, 0x02, 0x01, 0x02, 0x04, 0x04, 0x03, 0x04, 0x07, 0x05, 0x04, 0x04, 0x00, 0x01, 0x02, 0x77, 0x00, 0x01,
    0x02, 0x03, 0x11, 0x04, 0x05, 0x21, 0x31, 0x06, 0x12, 0x41, 0x51, 0x07, 0x61, 0x71, 0x13, 0x22, 0x32, 0x81, 0x08, 0x14, 0x42, 0x91, 0xa1, 0xb1, 0xc1, 0x09,
    0x23, 0x33, 0x52, 0xf0, 0x15, 0x62, 0x72, 0xd1, 0x0a, 0x16, 0x24, 0x34, 0xe1, 0x25, 0xf1, 0x17, 0x18, 0x19, 0x1a, 0x26, 0x27, 0x28, 0x29, 0x2a, 0x35, 0x36,
    0x37, 0x38, 0x39, 0x3a, 0x43, 0x44, 0x45, 0x46, 0x47, 0x48, 0x49, 0x4a, 0x53, 0x54, 0x55, 0x56, 0x57, 0x58, 0x59, 0x5a, 0x63, 0x64, 0x65, 0x66, 0x67, 0x68,
    0x69, 0x6a, 0x73, 0x74, 0x75, 0x76, 0x77, 0x78, 0x79, 0x7a, 0x82, 0x83, 0x84, 0x85, 0x86, 0x87, 0x88, 0x89, 0x8a, 0x92, 0x93, 0x94, 0x95, 0x96, 0x97, 0x98,
    0x99, 0x9a, 0xa2, 0xa3, 0xa4, 0xa5, 0xa6, 0xa7, 0xa8, 0xa9, 0xaa, 0xb2, 0xb3, 0xb4, 0xb5, 0xb6, 0xb7, 0xb8, 0xb9, 0xba, 0xc2, 0xc3, 0xc4, 0xc5, 0xc6, 0xc7,
    0xc8, 0xc9, 0xca, 0xd2, 0xd3, 0xd4, 0xd5, 0xd6, 0xd7, 0xd8, 0xd9, 0xda, 0xe2, 0xe3, 0xe4, 0xe5, 0xe6, 0xe7, 0xe8, 0xe9, 0xea, 0xf2, 0xf3, 0xf4, 0xf5, 0xf6,
    0xf7, 0xf8, 0xf9, 0xfa,
];

/// A JPEG into BGR. Most webcams leave the Huffman tables out of their MJPEG
/// frames; the standard ones go in then.
pub fn decode_jpeg(data: &[u8]) -> Option<Bgr> {
    let has_dht = data.windows(2).any(|w| w == [0xff, 0xc4]);
    let fixed;
    let data = if has_dht {
        data
    } else {
        let sos = data.windows(2).position(|w| w == [0xff, 0xda])?;
        let mut v = Vec::with_capacity(data.len() + DHT.len());
        v.extend_from_slice(&data[..sos]);
        v.extend_from_slice(DHT);
        v.extend_from_slice(&data[sos..]);
        fixed = v;
        &fixed
    };
    let opts = zune_jpeg::zune_core::options::DecoderOptions::default().jpeg_set_out_colorspace(zune_jpeg::zune_core::colorspace::ColorSpace::BGR);
    let mut dec = zune_jpeg::JpegDecoder::new_with_options(zune_jpeg::zune_core::bytestream::ZCursor::new(data), opts);
    let pixels = dec.decode().ok()?;
    let (w, h) = dec.dimensions()?;
    (pixels.len() == w * h * 3).then_some(Bgr { w, h, data: pixels })
}

/// A picture file, made no wider or taller than 640.
pub fn load_picture(path: &Path) -> Option<Bgr> {
    let img = image::open(path).ok()?.to_rgb8();
    let (w, h) = (img.width() as usize, img.height() as usize);
    let mut bgr = Bgr { w, h, data: img.pixels().flat_map(|p| [p[2], p[1], p[0]]).collect() };
    let scale = WIDTH as f64 / w.max(h) as f64;
    if scale < 1.0 {
        bgr = bgr.resize_area(((w as f64 * scale).round() as usize).max(1), ((h as f64 * scale).round() as usize).max(1));
    }
    Some(bgr)
}

impl Camera {
    /// `spec` is a /dev/video path, "auto", "file:<video>" or
    /// "images:<dir>".
    pub fn open(spec: &str) -> Result<Camera, String> {
        let now = Instant::now();
        let make = |source, infrared, description| Camera { source, start: now, next: now, infrared, description };
        if let Some(dir) = spec.strip_prefix("images:") {
            let mut files: Vec<PathBuf> = std::fs::read_dir(dir)
                .map(|d| d.filter_map(|e| e.ok().map(|e| e.path())).collect())
                .unwrap_or_default();
            files.retain(|p| p.extension().and_then(|e| e.to_str()).is_some_and(|e| matches!(e.to_lowercase().as_str(), "jpg" | "jpeg" | "png")));
            files.sort();
            let pictures: Vec<Bgr> = files.iter().filter_map(|f| load_picture(f)).collect();
            if pictures.is_empty() {
                return Err(format!("no pictures in {dir}"));
            }
            return Ok(make(Source::Images { pictures, index: 0, repeat: 0 }, false, format!("pictures in {dir}")));
        }
        if let Some(path) = spec.strip_prefix("file:") {
            return open_video(path).map(|(child, w, h, frame)| {
                make(Source::Video { child, w, h, frame }, false, Path::new(path).file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default())
            });
        }
        let path = if spec.is_empty() || spec == "auto" { auto_path() } else { spec.to_string() };
        if path.is_empty() {
            return Err("no camera found".into());
        }
        let info = probe(&path).ok_or_else(|| format!("{path} is not a camera"))?;
        if busy(&path) {
            return Err(format!("{path} is in use by another program"));
        }
        let dev = Device::open(&path, info.infrared).map_err(|e| format!("cannot open {path}: {e}"))?;
        Ok(make(Source::Device(dev), info.infrared, format!("{} ({path})", info.name)))
    }

    fn pace(&mut self, frame: Duration) {
        self.next += frame;
        let now = Instant::now();
        if self.next > now {
            std::thread::sleep(self.next - now);
        } else {
            self.next = now;
        }
    }

    /// Blocks until the next frame. The time is in milliseconds since the
    /// camera opened.
    pub fn read(&mut self) -> Option<(Bgr, f64)> {
        let frame = match &mut self.source {
            Source::Device(d) => d.read(),
            Source::Images { .. } => {
                self.pace(IMAGE_FRAME);
                let Source::Images { pictures, index, repeat } = &mut self.source else { unreachable!() };
                let f = pictures[*index].clone();
                *repeat += 1;
                if *repeat >= IMAGE_REPEAT {
                    *repeat = 0;
                    *index = (*index + 1) % pictures.len();
                }
                Some(f)
            }
            Source::Video { frame, .. } => {
                let d = *frame;
                self.pace(d);
                let Source::Video { child, w, h, .. } = &mut self.source else { unreachable!() };
                let mut buf = vec![0u8; *w * *h * 3];
                child.stdout.as_mut()?.read_exact(&mut buf).ok()?;
                Some(Bgr { w: *w, h: *h, data: buf })
            }
        }?;
        Some((frame, self.start.elapsed().as_secs_f64() * 1000.0))
    }
}

impl Drop for Camera {
    fn drop(&mut self) {
        if let Source::Video { child, .. } = &mut self.source {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

/// A video file through ffmpeg, as raw BGR frames no wider than 640.
fn open_video(path: &str) -> Result<(std::process::Child, usize, usize, Duration), String> {
    let probe = std::process::Command::new("ffprobe")
        .args(["-v", "error", "-select_streams", "v:0", "-show_entries", "stream=width,height,r_frame_rate", "-of", "csv=p=0", path])
        .output()
        .map_err(|_| "ffprobe is needed to read a video file".to_string())?;
    let text = String::from_utf8_lossy(&probe.stdout);
    let parts: Vec<&str> = text.trim().split(',').collect();
    let (w, h): (usize, usize) = match parts.as_slice() {
        [w, h, ..] => (w.parse().unwrap_or(0), h.parse().unwrap_or(0)),
        _ => (0, 0),
    };
    if w == 0 || h == 0 {
        return Err(format!("cannot open video file {path}"));
    }
    let fps = parts.get(2).and_then(|r| {
        let (n, d) = r.split_once('/')?;
        let (n, d): (f64, f64) = (n.parse().ok()?, d.parse().ok()?);
        (d > 0.0).then_some(n / d)
    });
    let fps = fps.filter(|f| *f > 1.0 && *f < 240.0).unwrap_or(30.0);
    let scale = (WIDTH as f64 / w.max(h) as f64).min(1.0);
    let (ow, oh) = (((w as f64 * scale) as usize / 2 * 2).max(2), ((h as f64 * scale) as usize / 2 * 2).max(2));
    let child = std::process::Command::new("ffmpeg")
        .args(["-v", "error", "-i", path, "-vf", &format!("scale={ow}:{oh}"), "-f", "rawvideo", "-pix_fmt", "bgr24", "-"])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map_err(|_| "ffmpeg is needed to read a video file".to_string())?;
    Ok((child, ow, oh, Duration::from_secs_f64(1.0 / fps)))
}
