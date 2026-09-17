//! V05: MP4/MOV container metadata without a decoder dependency.
//!
//! Reads just enough of the box tree for the catalog: duration (mvhd),
//! dimensions (first video tkhd), codec fourcc (stsd), and capture time
//! (mvhd creation, Mac epoch). Everything is bounds-checked; corrupt or
//! truncated files yield `None`, never a panic. Posters come from system
//! helpers at import time; playback is external (see `poster_for`).

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct VideoMeta {
    pub duration_ms: u64,
    pub width: u32,
    pub height: u32,
    /// Codec fourcc, e.g. `avc1`.
    pub codec: String,
    /// `YYYY:MM:DD HH:MM:SS` from mvhd creation, or empty when unset.
    pub captured_at: String,
}

struct Cursor<'a> {
    b: &'a [u8],
    pos: usize,
}

impl<'a> Cursor<'a> {
    fn rest(&self) -> usize {
        self.b.len().saturating_sub(self.pos)
    }

    fn u8(&mut self) -> Option<u8> {
        let v = *self.b.get(self.pos)?;
        self.pos += 1;
        Some(v)
    }

    fn u32(&mut self) -> Option<u32> {
        if self.rest() < 4 {
            return None;
        }
        let v = u32::from_be_bytes([
            self.b[self.pos],
            self.b[self.pos + 1],
            self.b[self.pos + 2],
            self.b[self.pos + 3],
        ]);
        self.pos += 4;
        Some(v)
    }

    fn u64(&mut self) -> Option<u64> {
        if self.rest() < 8 {
            return None;
        }
        let mut a = [0u8; 8];
        a.copy_from_slice(&self.b[self.pos..self.pos + 8]);
        self.pos += 8;
        Some(u64::from_be_bytes(a))
    }

    fn tag(&mut self) -> Option<[u8; 4]> {
        if self.rest() < 4 {
            return None;
        }
        let t = [
            self.b[self.pos],
            self.b[self.pos + 1],
            self.b[self.pos + 2],
            self.b[self.pos + 3],
        ];
        self.pos += 4;
        Some(t)
    }

    fn skip(&mut self, n: usize) {
        self.pos = (self.pos + n).min(self.b.len());
    }
}

/// Box header at `pos`: (fourcc, content start, content end, next box).
/// Handles `size == 0` (to end of parent) and `size == 1` (largesize).
fn header(b: &[u8], pos: usize, parent_end: usize) -> Option<([u8; 4], usize, usize, usize)> {
    if pos + 8 > parent_end || pos + 8 > b.len() {
        return None;
    }
    let size = u32::from_be_bytes([b[pos], b[pos + 1], b[pos + 2], b[pos + 3]]) as u64;
    let tag = [b[pos + 4], b[pos + 5], b[pos + 6], b[pos + 7]];
    let (content, mut next) = if size == 1 {
        if pos + 16 > parent_end || pos + 16 > b.len() {
            return None;
        }
        let mut a = [0u8; 8];
        a.copy_from_slice(&b[pos + 8..pos + 16]);
        let big = u64::from_be_bytes(a);
        (
            pos + 16,
            pos.saturating_add(big.min(usize::MAX as u64) as usize),
        )
    } else if size == 0 {
        (pos + 8, parent_end)
    } else {
        (pos + 8, pos + size as usize)
    };
    next = next.min(parent_end).min(b.len());
    if content > next {
        return None;
    }
    Some((tag, content, next, next))
}

/// Walk child boxes of `[start, end)`, calling `f` per child. Stops on
/// malformed headers instead of spinning.
fn children(b: &[u8], start: usize, end: usize, mut f: impl FnMut([u8; 4], usize, usize)) {
    let mut pos = start;
    while pos + 8 <= end.min(b.len()) {
        let Some((tag, content, _child_end, next)) = header(b, pos, end) else {
            break;
        };
        if next <= pos {
            break;
        }
        f(tag, content, _child_end.min(b.len()));
        pos = next;
    }
}

fn fixed1616(b: &[u8], at: usize) -> Option<u32> {
    if at + 4 > b.len() {
        return None;
    }
    Some(u32::from_be_bytes([b[at], b[at + 1], b[at + 2], b[at + 3]]) >> 16)
}

/// Mac-epoch (1904) seconds → `YYYY:MM:DD HH:MM:SS`; 0/None → empty.
fn mac_time(secs: u64) -> String {
    if secs == 0 {
        return String::new();
    }
    const MAC_TO_UNIX: i64 = 2_082_844_800;
    let unix = secs as i64 - MAC_TO_UNIX;
    if unix < 0 {
        return String::new();
    }
    let days = unix.div_euclid(86400);
    let sec = unix.rem_euclid(86400);
    let z = days + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!(
        "{y:04}:{m:02}:{d:02} {:02}:{:02}:{:02}",
        sec / 3600,
        sec / 60 % 60,
        sec % 60
    )
}

fn parse_mvhd(b: &[u8], content: usize, meta: &mut VideoMeta) {
    let mut c = Cursor { b, pos: content };
    let version = c.u8().unwrap_or(1);
    c.skip(3);
    if version == 1 {
        let created = c.u64().unwrap_or(0);
        c.u64();
        let timescale = c.u32().unwrap_or(0);
        let duration = c.u64().unwrap_or(0);
        meta.captured_at = mac_time(created);
        if timescale > 0 {
            // u128: an all-ones "unknown" v1 duration must not overflow.
            meta.duration_ms =
                (duration as u128 * 1000 / timescale as u128).min(u64::MAX as u128) as u64;
        }
    } else {
        let created = c.u32().unwrap_or(0) as u64;
        c.u32();
        let timescale = c.u32().unwrap_or(0);
        let duration = c.u32().unwrap_or(0) as u64;
        meta.captured_at = mac_time(created);
        if timescale > 0 {
            meta.duration_ms = duration * 1000 / timescale as u64;
        }
    }
}

fn parse_tkhd(b: &[u8], content: usize, meta: &mut VideoMeta) {
    if meta.width > 0 {
        return; // first video track wins
    }
    let mut c = Cursor { b, pos: content };
    let version = c.u8().unwrap_or(1);
    c.skip(3);
    let (w_at, h_at) = if version == 1 {
        c.skip(8 + 8 + 4 + 4 + 8 + 8 + 2 + 2 + 2 + 2 + 36);
        (c.pos, c.pos + 4)
    } else {
        c.skip(4 + 4 + 4 + 4 + 4 + 8 + 2 + 2 + 2 + 2 + 36);
        (c.pos, c.pos + 4)
    };
    if let (Some(w), Some(h)) = (fixed1616(b, w_at), fixed1616(b, h_at)) {
        if w > 0 {
            // Display matrix {a b u / c d v / x y w} precedes the size. A
            // quarter turn (a = d = 0; phone portrait clips) swaps the
            // displayed width and height.
            let m = |i: usize| {
                w_at.checked_sub(36 - i * 4)
                    .and_then(|at| b.get(at..at + 4))
                    .map(|s| i32::from_be_bytes([s[0], s[1], s[2], s[3]]))
            };
            let quarter = m(0) == Some(0) && m(4) == Some(0) && m(1).is_some_and(|v| v != 0);
            (meta.width, meta.height) = if quarter { (h, w) } else { (w, h) };
        }
    }
}

fn parse_stsd(b: &[u8], content: usize, meta: &mut VideoMeta) {
    if !meta.codec.is_empty() {
        return;
    }
    let mut c = Cursor { b, pos: content };
    c.skip(4); // version/flags
    let count = c.u32().unwrap_or(0);
    if count == 0 || c.rest() < 8 {
        return;
    }
    c.skip(4); // entry size
    if let Some(tag) = c.tag() {
        let s = String::from_utf8_lossy(&tag).into_owned();
        if s.chars().all(|ch| !ch.is_control()) {
            meta.codec = s;
        }
    }
}

fn parse_minf(b: &[u8], content: usize, end: usize, meta: &mut VideoMeta) {
    children(b, content, end, |tag, c, e| {
        if &tag == b"stbl" {
            children(b, c, e, |tag, c, _| {
                if &tag == b"stsd" {
                    parse_stsd(b, c, meta);
                }
            });
        }
    });
}

fn parse_trak(b: &[u8], content: usize, end: usize, meta: &mut VideoMeta) {
    children(b, content, end, |tag, c, e| match &tag {
        t if t == b"tkhd" => parse_tkhd(b, c, meta),
        t if t == b"mdia" => {
            // The codec comes from the video track only: `hdlr` precedes
            // `minf`, and an audio-first file must not report `mp4a`.
            let mut video = true;
            children(b, c, e, |tag, c, e| {
                if &tag == b"hdlr" {
                    video = b.get(c + 8..c + 12) == Some(b"vide".as_slice());
                } else if &tag == b"minf" && video {
                    parse_minf(b, c, e, meta);
                }
            })
        }
        _ => {}
    });
}

fn parse_moov(b: &[u8], content: usize, end: usize, meta: &mut VideoMeta) {
    children(b, content, end, |tag, c, e| match &tag {
        t if t == b"mvhd" => parse_mvhd(b, c, meta),
        t if t == b"trak" => parse_trak(b, c, e, meta),
        _ => {}
    });
}

/// Parse raw file bytes. Requires an `ftyp` box up front (real containers),
/// then reads the first `moov`.
pub fn parse_bytes(b: &[u8]) -> Option<VideoMeta> {
    let mut meta = VideoMeta::default();
    let mut found_ftyp = false;
    let mut pos = 0;
    while pos + 8 <= b.len() {
        let Some((tag, content, end, next)) = header(b, pos, b.len()) else {
            break;
        };
        if &tag == b"ftyp" {
            found_ftyp = true;
        } else if &tag == b"moov" {
            parse_moov(b, content, end, &mut meta);
            break;
        }
        let _ = content;
        let _ = end;
        pos = next;
    }
    if !found_ftyp {
        return None;
    }
    Some(meta)
}

/// Parse a video file from disk. Top-level boxes are walked by seeking, so
/// only `ftyp` and `moov` are read — never the (multi-GB) `mdat` payload —
/// wherever `moov` sits (faststart head or recorder tail).
pub fn read(path: &std::path::Path) -> Option<VideoMeta> {
    use std::io::{Read, Seek, SeekFrom};
    // Sample tables of very long recordings stay well below this; the
    // headers parse_bytes needs (mvhd, first trak) come first anyway.
    const BOX_CAP: u64 = 64 << 20;
    let mut f = std::fs::File::open(path).ok()?;
    let len = f.metadata().ok()?.len();
    let mut bytes = Vec::new();
    let mut pos = 0u64;
    while pos + 8 <= len {
        let mut hdr = [0u8; 16];
        let avail = (len - pos).min(16) as usize;
        f.seek(SeekFrom::Start(pos)).ok()?;
        f.read_exact(&mut hdr[..avail]).ok()?;
        let size = match u32::from_be_bytes([hdr[0], hdr[1], hdr[2], hdr[3]]) {
            0 => len - pos,
            1 if avail == 16 => u64::from_be_bytes(hdr[8..16].try_into().ok()?),
            1 => break,
            s => s as u64,
        };
        if size < 8 {
            break;
        }
        let end = pos.saturating_add(size).min(len);
        let tag = &hdr[4..8];
        if tag == b"ftyp" || tag == b"moov" {
            let take = (end - pos).min(BOX_CAP);
            f.seek(SeekFrom::Start(pos)).ok()?;
            let start = bytes.len();
            (&mut f).take(take).read_to_end(&mut bytes).ok()?;
            if tag == b"moov" {
                break;
            }
            if bytes.len() - start < take as usize {
                break;
            }
        }
        pos = end;
    }
    parse_bytes(&bytes)
}

/// Run a helper with a timeout (system thumbnailers can stall
/// headless). On timeout the child is abandoned and `None` returns.
fn run_helper_timeout(mut cmd: std::process::Command, secs: u64) -> Option<std::process::Output> {
    use std::sync::mpsc;
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let _ = tx.send(cmd.output());
    });
    rx.recv_timeout(std::time::Duration::from_secs(secs))
        .ok()
        .and_then(|r| r.ok())
}

/// Best-effort poster JPEG via system helpers (macOS QuickLook, else
/// ffmpeg). `None` means "use the placeholder tile" — never fatal.
/// Helpers run under a 20 s timeout (they can stall headless), and
/// `LAIKA_NO_POSTER=1` skips them entirely (tests, hermetic environments).
pub fn poster_for(path: &std::path::Path) -> Option<Vec<u8>> {
    if std::env::var("LAIKA_NO_POSTER").as_deref() == Ok("1") {
        return None;
    }
    // One directory per call: concurrent imports must never pick up (or
    // delete) each other's poster output.
    static SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let tag = SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let out_dir = std::env::temp_dir().join(format!("laika-poster-{}-{tag}", std::process::id()));
    std::fs::create_dir_all(&out_dir).ok()?;
    #[cfg(target_os = "macos")]
    {
        let mut cmd = std::process::Command::new("qlmanage");
        cmd.arg("-t")
            .arg("-s")
            .arg("1024")
            .arg("-o")
            .arg(&out_dir)
            .arg(path);
        let ok = run_helper_timeout(cmd, 20).is_some_and(|o| o.status.success());
        // qlmanage names the output after the source file.
        let produced = ok
            .then(|| {
                std::fs::read_dir(&out_dir)
                    .ok()?
                    .filter_map(|e| e.ok())
                    .find(|e| {
                        matches!(
                            e.path().extension().and_then(|x| x.to_str()),
                            Some("jpg") | Some("png")
                        )
                    })
            })
            .flatten();
        let _ = tag;
        if let Some(entry) = produced {
            let bytes = std::fs::read(entry.path()).ok();
            std::fs::remove_dir_all(&out_dir).ok();
            return bytes;
        }
        std::fs::remove_dir_all(&out_dir).ok();
        return None;
    }
    #[cfg(not(target_os = "macos"))]
    {
        let out = out_dir.join(format!("p{tag}.jpg"));
        let mut cmd = std::process::Command::new("ffmpeg");
        cmd.arg("-y")
            .arg("-v")
            .arg("error")
            .arg("-i")
            .arg(path)
            .arg("-vframes")
            .arg("1")
            .arg(&out);
        let ok = run_helper_timeout(cmd, 20).is_some_and(|o| o.status.success() && out.exists());
        if ok {
            let bytes = std::fs::read(&out).ok();
            std::fs::remove_dir_all(&out_dir).ok();
            return bytes;
        }
        std::fs::remove_dir_all(&out_dir).ok();
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn box_bytes(tag: &[u8; 4], payload: &[u8]) -> Vec<u8> {
        let mut v = Vec::new();
        v.extend_from_slice(&((8 + payload.len()) as u32).to_be_bytes());
        v.extend_from_slice(tag);
        v.extend_from_slice(payload);
        v
    }

    fn mvhd_v0(timescale: u32, duration: u32, created_mac: u32) -> Vec<u8> {
        let mut p = vec![0u8]; // version
        p.extend_from_slice(&[0, 0, 0]); // flags
        p.extend_from_slice(&created_mac.to_be_bytes());
        p.extend_from_slice(&0u32.to_be_bytes()); // modified
        p.extend_from_slice(&timescale.to_be_bytes());
        p.extend_from_slice(&duration.to_be_bytes());
        p.extend_from_slice(&[0u8; 80]); // rest
        p
    }

    fn tkhd_v0(w: u32, h: u32) -> Vec<u8> {
        let mut p = vec![0u8];
        p.extend_from_slice(&[0, 0, 0]);
        p.extend_from_slice(&[0u8; 4 + 4 + 4 + 4 + 4 + 8 + 2 + 2 + 2 + 2]); // to matrix
        p.extend_from_slice(&[0u8; 36]); // matrix
        p.extend_from_slice(&((w << 16) as u32).to_be_bytes());
        p.extend_from_slice(&((h << 16) as u32).to_be_bytes());
        p
    }

    fn stsd(codec: &[u8; 4]) -> Vec<u8> {
        let mut p = vec![0, 0, 0, 0]; // version/flags
        p.extend_from_slice(&1u32.to_be_bytes()); // entry count
        p.extend_from_slice(&86u32.to_be_bytes()); // entry size
        p.extend_from_slice(codec);
        p.extend_from_slice(&[0u8; 78]);
        p
    }

    fn fixture() -> Vec<u8> {
        let minf = box_bytes(
            b"minf",
            &box_bytes(b"stbl", &box_bytes(b"stsd", &stsd(b"avc1"))),
        );
        let mdia = box_bytes(b"mdia", &box_bytes(b"minf", &minf[8..]));
        let trak = {
            let mut p = box_bytes(b"tkhd", &tkhd_v0(1920, 1080));
            p.extend_from_slice(&mdia);
            box_bytes(b"trak", &p)
        };
        let mut moov_payload = box_bytes(b"mvhd", &mvhd_v0(1000, 42150, 0));
        moov_payload.extend_from_slice(&trak);
        let ftyp_payload = b"isom\0\0\0\0isom".to_vec();
        let mut file = box_bytes(b"ftyp", &ftyp_payload);
        file.extend_from_slice(&box_bytes(b"moov", &moov_payload));
        file
    }

    #[test]
    fn parses_duration_dims_codec() {
        let m = parse_bytes(&fixture()).expect("fixture parses");
        assert_eq!(m.duration_ms, 42150);
        assert_eq!((m.width, m.height), (1920, 1080));
        assert_eq!(m.codec, "avc1");
    }

    #[test]
    fn rejects_garbage_and_truncation() {
        assert!(parse_bytes(b"").is_none());
        assert!(parse_bytes(b"not a movie at all........").is_none());
        // ftyp without moov: valid prefix, no metadata.
        let f = box_bytes(b"ftyp", b"isom\0\0\0\0");
        let m = parse_bytes(&f).expect("ftyp-only parses");
        assert_eq!(m.duration_ms, 0);
        // Truncated moov never panics.
        let mut t = fixture();
        t.truncate(t.len() / 2);
        let _ = parse_bytes(&t);
        // Unknown boxes are skipped.
        let mut u = box_bytes(b"ftyp", b"isom\0\0\0\0");
        u.extend_from_slice(&box_bytes(b"free", &[9u8; 40]));
        u.extend_from_slice(&box_bytes(
            b"moov",
            &box_bytes(b"mvhd", &mvhd_v0(25, 250, 0)),
        ));
        let m = parse_bytes(&u).expect("skips free");
        assert_eq!(m.duration_ms, 10_000);
    }

    #[test]
    fn read_finds_moov_after_large_mdat_and_rotation_swaps() {
        // Recorder layout: ftyp, a >32 MiB mdat, then moov at the tail.
        let dir = std::env::temp_dir().join(format!("laika-video-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("tail.mov");
        let mut tkhd = tkhd_v0(1920, 1080);
        // Quarter-turn display matrix: a = 0, b = 1.0, c = -1.0, d = 0.
        let m = tkhd.len() - 8 - 36;
        tkhd[m + 4..m + 8].copy_from_slice(&0x0001_0000u32.to_be_bytes());
        tkhd[m + 12..m + 16].copy_from_slice(&0xFFFF_0000u32.to_be_bytes());
        let trak = box_bytes(b"trak", &box_bytes(b"tkhd", &tkhd));
        let mut moov = box_bytes(b"mvhd", &mvhd_v0(600, 6000, 0));
        moov.extend_from_slice(&trak);
        let mut file = box_bytes(b"ftyp", b"qt  \0\0\0\0qt  ");
        let mdat_len: u32 = (40 << 20) + 8;
        file.extend_from_slice(&mdat_len.to_be_bytes());
        file.extend_from_slice(b"mdat");
        std::fs::write(&p, &file).unwrap();
        {
            use std::io::Write;
            let f = std::fs::OpenOptions::new().append(true).open(&p).unwrap();
            f.set_len(file.len() as u64 + (40 << 20)).unwrap();
            let mut f = std::fs::OpenOptions::new().append(true).open(&p).unwrap();
            f.write_all(&box_bytes(b"moov", &moov)).unwrap();
        }
        let m = read(&p).expect("tail moov parses");
        assert_eq!(m.duration_ms, 10_000);
        assert_eq!((m.width, m.height), (1080, 1920));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn huge_v1_duration_does_not_overflow() {
        let mut p = vec![1u8, 0, 0, 0];
        p.extend_from_slice(&[0u8; 16]); // created + modified
        p.extend_from_slice(&1u32.to_be_bytes()); // timescale
        p.extend_from_slice(&u64::MAX.to_be_bytes()); // unknown duration
        p.extend_from_slice(&[0u8; 80]);
        let mut file = box_bytes(b"ftyp", b"isom\0\0\0\0");
        file.extend_from_slice(&box_bytes(b"moov", &box_bytes(b"mvhd", &p)));
        let m = parse_bytes(&file).expect("parses");
        assert_eq!(m.duration_ms, u64::MAX);
    }

    #[test]
    fn mac_epoch_maps_to_exif_shape() {
        // 2026-06-14 18:42:00 UTC → back to EXIF shape.
        let unix: i64 = 1781462520;
        let mac = (unix + 2_082_844_800) as u64;
        assert_eq!(mac_time(mac), "2026:06:14 18:42:00");
        assert_eq!(mac_time(0), "");
    }
}
