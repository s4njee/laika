//! Background backup of originals + sidecars (Phase 4).
//!
//! Targets: a Linux box over SFTP (system `ssh`, key auth), a mounted
//! SMB/NFS share, or S3-compatible storage. Every target uses the same key
//! layout and only counts a file as backed up after verifying the stored
//! bytes (S3: blake3 metadata; SFTP: remote sha256; share: blake3 re-read).
//!
//! Queue worker uploads originals and sidecars with key layout
//! `<catalog>/<yyyy>/<yyyy-mm-dd>/<filename>`, then verifies with a
//! metadata-only ranged GET comparing the stored blake3. Only verified
//! objects flip the photo to `synced`.
//!
//! No tokio in the main tree: each upload builds a short-lived
//! current-thread runtime on a GPUI background thread (plan's tokio runtime,
//! scoped to the transfer).

use std::collections::HashMap;
use std::path::Path;

use object_store::aws::{AmazonS3, AmazonS3Builder};
use object_store::path::Path as StorePath;
use object_store::{
    Attribute, Attributes, GetOptions, GetRange, ObjectStore, PutOptions, PutPayload,
};

#[cfg(test)]
use crate::photo::SyncState;

pub const BLAKE_ATTR: &str = "blake3";

/// Where backups go. The Linux-server targets (SFTP, share) are the default.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum BackupTarget {
    #[default]
    Sftp,
    Share,
    S3,
}

impl BackupTarget {
    pub fn key(self) -> &'static str {
        match self {
            BackupTarget::Sftp => "sftp",
            BackupTarget::Share => "share",
            BackupTarget::S3 => "s3",
        }
    }

    pub fn from_key(k: &str) -> Option<Self> {
        match k {
            "sftp" => Some(BackupTarget::Sftp),
            "share" => Some(BackupTarget::Share),
            "s3" => Some(BackupTarget::S3),
            _ => None,
        }
    }
}

/// Network share protocol (only affects the Connect URL; transfers go
/// through the mounted folder either way).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ShareKind {
    #[default]
    Smb,
    Nfs,
}

impl ShareKind {
    pub fn key(self) -> &'static str {
        match self {
            ShareKind::Smb => "smb",
            ShareKind::Nfs => "nfs",
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct SyncSettings {
    pub target: BackupTarget,
    // S3-compatible storage.
    pub endpoint: String,
    pub bucket: String,
    pub region: String,
    pub access_key: String,
    // SFTP to a Linux box (system ssh; keys from ssh-agent / ~/.ssh).
    pub sftp_host: String,
    pub sftp_port: String,
    pub sftp_user: String,
    /// Remote base folder (absolute, or relative to the login's home).
    pub sftp_path: String,
    /// Optional private key file; empty uses ssh-agent / default keys.
    pub sftp_identity: String,
    // SMB/NFS share mounted on this Mac.
    pub share_kind: ShareKind,
    pub share_server: String,
    /// SMB share name or NFS export path.
    pub share_name: String,
    /// Local folder of the mounted share (e.g. `/Volumes/backup/laika`).
    pub share_path: String,
}

impl SyncSettings {
    /// The active target has everything it needs (S3 still needs its
    /// secret, checked by the caller).
    pub fn configured(&self) -> bool {
        match self.target {
            BackupTarget::S3 => self.s3_configured(),
            BackupTarget::Sftp => {
                !self.sftp_host.trim().is_empty() && !self.sftp_path.trim().is_empty()
            }
            BackupTarget::Share => !self.share_path.trim().is_empty(),
        }
    }

    pub fn s3_configured(&self) -> bool {
        !self.endpoint.is_empty() && !self.bucket.is_empty() && !self.access_key.is_empty()
    }

    /// Whether the active target needs a keychain secret.
    pub fn needs_secret(&self) -> bool {
        self.target == BackupTarget::S3
    }

    /// Short human description of the active destination.
    pub fn describe(&self) -> String {
        match self.target {
            BackupTarget::S3 => format!("s3://{}", self.bucket),
            BackupTarget::Sftp => format!(
                "sftp://{}{}{}/{}",
                if self.sftp_user.is_empty() {
                    String::new()
                } else {
                    format!("{}@", self.sftp_user)
                },
                self.sftp_host,
                match self.sftp_port.trim() {
                    "" | "22" => String::new(),
                    p => format!(":{p}"),
                },
                self.sftp_path.trim_start_matches('/')
            ),
            BackupTarget::Share => self.share_path.clone(),
        }
    }

    /// `smb://server/share` or `nfs://server/export` for macOS to mount.
    pub fn share_url(&self) -> Option<String> {
        let server = self.share_server.trim();
        let name = self.share_name.trim().trim_start_matches('/');
        if server.is_empty() || name.is_empty() {
            return None;
        }
        Some(format!("{}://{server}/{name}", self.share_kind.key()))
    }

    /// Likely mounted location after asking the OS to connect.
    pub fn share_mount_guess(&self) -> Option<std::path::PathBuf> {
        let name = self.share_name.trim().trim_end_matches('/');
        let last = name.rsplit('/').next().filter(|s| !s.is_empty())?;
        #[cfg(target_os = "macos")]
        return Some(std::path::Path::new("/Volumes").join(last));
        #[cfg(target_os = "windows")]
        {
            if self.share_kind == ShareKind::Smb && !self.share_server.trim().is_empty() {
                return Some(std::path::PathBuf::from(format!(
                    r"\\{}\{}",
                    self.share_server.trim(),
                    name.replace('/', r"\")
                )));
            }
            return None;
        }
        #[cfg(all(unix, not(target_os = "macos")))]
        {
            let _ = last;
            None
        }
    }

    /// Overlay `LAIKA_S3_*` env vars (local-dev path without UI editing).
    pub fn apply_env(&mut self) {
        if let Ok(v) = std::env::var("LAIKA_S3_ENDPOINT") {
            if !v.is_empty() {
                self.endpoint = v;
            }
        }
        if let Ok(v) = std::env::var("LAIKA_S3_BUCKET") {
            if !v.is_empty() {
                self.bucket = v;
            }
        }
        if let Ok(v) = std::env::var("LAIKA_S3_REGION") {
            if !v.is_empty() {
                self.region = v;
            }
        }
        if let Ok(v) = std::env::var("LAIKA_S3_KEY") {
            if !v.is_empty() {
                self.access_key = v;
            }
        }
    }

    pub fn secret(&self) -> Option<String> {
        // V32: whatever secret is in use is scrubbed from every log line.
        if let Ok(s) = std::env::var("LAIKA_S3_SECRET") {
            if !s.is_empty() {
                crate::logging::register_secret(&s);
                return Some(s);
            }
        }
        let s = keyring::Entry::new("laika", &format!("s3-secret/{}", self.access_key))
            .ok()?
            .get_password()
            .ok()?;
        crate::logging::register_secret(&s);
        Some(s)
    }

    pub fn store_secret(&self, secret: &str) -> Result<(), String> {
        keyring::Entry::new("laika", &format!("s3-secret/{}", self.access_key))
            .map_err(|e| e.to_string())?
            .set_password(secret)
            .map_err(|e| e.to_string())
    }
}

/// `(yyyy, yyyy-mm-dd)` from EXIF `captured_at` (`YYYY:MM:DD …` or
/// `YYYY-MM-DD…`), file mtime, or a stable unknown marker.
pub fn date_prefix(captured_at: &str, path: &Path) -> (String, String) {
    // `get` (not slicing): garbage EXIF with a multi-byte char inside the
    // first 10 bytes must fall through, never panic.
    if let Some(d) = captured_at.get(..10) {
        let norm = d.replace(':', "-");
        let parts: Vec<&str> = norm.split('-').collect();
        if parts.len() == 3
            && parts[0].len() == 4
            && parts[1].len() == 2
            && parts[2].len() == 2
            && parts.iter().all(|p| p.bytes().all(|b| b.is_ascii_digit()))
        {
            return (parts[0].to_string(), norm);
        }
    }
    if let Ok(meta) = std::fs::metadata(path) {
        if let Ok(mtime) = meta.modified() {
            let secs = mtime
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0);
            // Days since epoch → civil date (Howard Hinnant's algorithm).
            let z = (secs / 86400) as i64 + 719468;
            let era = if z >= 0 { z } else { z - 146096 } / 146097;
            let doe = z - era * 146097;
            let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
            let y = yoe + era * 400;
            let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
            let mp = (5 * doy + 2) / 153;
            let d = doy - (153 * mp + 2) / 5 + 1;
            let m = if mp < 10 { mp + 3 } else { mp - 9 };
            let y = if m <= 2 { y + 1 } else { y };
            return (format!("{y:04}"), format!("{y:04}-{m:02}-{d:02}"));
        }
    }
    ("unknown".into(), "unknown-date".into())
}

/// S3-safe catalog prefix: lowercase slug, runs of other chars to `-`.
pub fn sanitize_catalog(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    let mut dash = false;
    for c in name.chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
            dash = false;
        } else if !dash && !out.is_empty() {
            out.push('-');
            dash = true;
        }
    }
    let s = out.trim_matches('-').to_string();
    if s.is_empty() { "laika".into() } else { s }
}

/// `<catalog>/<yyyy>/<yyyy-mm-dd>/<filename>`; sidecars append `.xmp`.
pub fn remote_key(catalog: &str, captured_at: &str, path: &Path, sidecar: bool) -> String {
    let catalog = sanitize_catalog(catalog);
    let (yyyy, ymd) = date_prefix(captured_at, path);
    let filename = path.file_name().and_then(|n| n.to_str()).unwrap_or("file");
    let base = format!("{catalog}/{yyyy}/{ymd}/{filename}");
    if sidecar { format!("{base}.xmp") } else { base }
}

/// Immutable, idempotent backup key. The content hash prevents two
/// same-named files from silently overwriting each other and keeps prior
/// sidecar revisions recoverable. Reconnecting uploads the same bytes to
/// the same key instead of creating a duplicate.
pub fn versioned_remote_key(
    catalog: &str,
    captured_at: &str,
    path: &Path,
    sidecar: bool,
    hash: &str,
) -> String {
    let base = remote_key(catalog, captured_at, path, sidecar);
    let version: String = hash
        .chars()
        .filter(|c| c.is_ascii_hexdigit())
        .take(64)
        .collect();
    if version.is_empty() {
        return base;
    }
    match base.rsplit_once('/') {
        Some((dir, file)) => format!("{dir}/{version}-{file}"),
        None => format!("{version}-{base}"),
    }
}

pub fn build_store(settings: &SyncSettings, secret: &str) -> Result<AmazonS3, String> {
    AmazonS3Builder::new()
        .with_endpoint(&settings.endpoint)
        .with_region(&settings.region)
        .with_bucket_name(&settings.bucket)
        .with_access_key_id(&settings.access_key)
        .with_secret_access_key(secret)
        .with_allow_http(true)
        .build()
        .map_err(|e| e.to_string())
}

fn put_options(hash: &str) -> PutOptions {
    let mut attrs: HashMap<Attribute, String> = HashMap::new();
    attrs.insert(Attribute::Metadata(BLAKE_ATTR.into()), hash.to_string());
    PutOptions {
        attributes: Attributes::from_iter(attrs),
        ..Default::default()
    }
}

/// Verify the stored blake3 metadata with a metadata-only ranged GET,
/// without re-uploading. `synced` only after this passes.
pub async fn verify_remote(
    store: &impl ObjectStore,
    remote: &str,
    expected_hash: &str,
) -> Result<(), String> {
    let path = StorePath::from(remote);
    let opts = GetOptions {
        range: Some(GetRange::Bounded(0..1)),
        ..Default::default()
    };
    let got = store
        .get_opts(&path, opts)
        .await
        .map_err(|e| format!("verify {remote}: {e}"))?;
    let stored = got
        .attributes
        .get(&Attribute::Metadata(BLAKE_ATTR.into()))
        .map(|s| s.to_string())
        .unwrap_or_default();
    if stored != expected_hash {
        return Err(format!(
            "hash mismatch for {remote}: stored {stored:?} != local {expected_hash}"
        ));
    }
    Ok(())
}

/// Upload one local file and verify the stored hash with a metadata-only
/// ranged GET. Returns uploaded bytes.
pub async fn upload_and_verify(
    store: &AmazonS3,
    local: &Path,
    remote: &str,
    hash: &str,
) -> Result<u64, String> {
    let bytes = std::fs::read(local).map_err(|e| format!("read {}: {e}", local.display()))?;
    let len = bytes.len() as u64;
    let path = StorePath::from(remote);
    store
        .put_opts(
            &path,
            PutPayload::from_bytes(bytes.into()),
            put_options(hash),
        )
        .await
        .map_err(|e| format!("put {remote}: {e}"))?;
    verify_remote(store, remote, hash).await?;
    Ok(len)
}

/// Upload one file to the active target and verify it. Returns bytes sent.
pub fn upload_blocking(
    settings: &SyncSettings,
    secret: &str,
    local: &Path,
    remote: &str,
    hash: &str,
) -> Result<u64, String> {
    match settings.target {
        BackupTarget::S3 => s3_upload_blocking(settings, secret, local, remote, hash),
        BackupTarget::Sftp => sftp::upload(settings, local, remote),
        BackupTarget::Share => share::upload(settings, local, remote),
    }
}

/// Restore one missing original from SFTP to its catalog path. Existing
/// destination files are never replaced, and the temporary download is only
/// installed after its BLAKE3 matches the import-time catalog checksum.
pub fn restore_blocking(
    settings: &SyncSettings,
    remote: &str,
    destination: &Path,
    expected_blake3: &str,
) -> Result<u64, String> {
    match settings.target {
        BackupTarget::Sftp => sftp::download(settings, remote, destination, expected_blake3),
        _ => Err("pull restore is currently available for SFTP backups only".to_string()),
    }
}

/// Connectivity + write check for the active target.
pub fn check_blocking(settings: &SyncSettings, secret: &str) -> Result<(), String> {
    match settings.target {
        BackupTarget::S3 => s3_check_blocking(settings, secret),
        BackupTarget::Sftp => sftp::check(settings),
        BackupTarget::Share => share::check(settings),
    }
}

/// Blocking upload+verify on a short-lived current-thread runtime.
fn s3_upload_blocking(
    settings: &SyncSettings,
    secret: &str,
    local: &Path,
    remote: &str,
    hash: &str,
) -> Result<u64, String> {
    let store = build_store(settings, secret)?;
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|e| e.to_string())?;
    rt.block_on(upload_and_verify(&store, local, remote, hash))
}

/// Blocking connectivity check: build the store and list one key prefix.
fn s3_check_blocking(settings: &SyncSettings, secret: &str) -> Result<(), String> {
    let store = build_store(settings, secret)?;
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|e| e.to_string())?;
    rt.block_on(async {
        use futures::StreamExt;
        let mut list = store.list(None);
        match list.next().await {
            Some(Ok(_)) | None => Ok(()),
            Some(Err(e)) => Err(e.to_string()),
        }
    })
}

/// SFTP-style backup over the system `ssh` client. Each file streams into
/// a temp name through `ssh host sh -c`, is renamed into place, and the
/// remote `sha256sum` must match the sha256 computed while streaming.
/// Connections are multiplexed (ControlMaster) so files after the first
/// skip the handshake. Auth is non-interactive: ssh-agent, default keys,
/// or the configured identity file.
pub mod sftp {
    use super::SyncSettings;
    use sha2::{Digest, Sha256};
    use std::io::{Read, Write};
    use std::path::Path;
    use std::process::{Command, Stdio};

    /// POSIX single-quote a string for the remote shell.
    pub fn quote(s: &str) -> String {
        format!("'{}'", s.replace('\'', "'\\''"))
    }

    /// Remote path for a key under the base folder. `~`/`~/…` become
    /// home-relative (ssh commands start in the login's home), so no
    /// unquoted tilde expansion is needed.
    pub fn remote_path(base: &str, key: &str) -> String {
        let base = base.trim().trim_end_matches('/');
        let base = if base == "~" {
            ""
        } else {
            base.strip_prefix("~/").unwrap_or(base)
        };
        let key = key.trim_start_matches('/');
        if base.is_empty() {
            key.to_string()
        } else {
            format!("{base}/{key}")
        }
    }

    /// Shell script run on the server for one upload.
    pub fn upload_script(final_path: &str) -> String {
        let dir = match final_path.rsplit_once('/') {
            Some((d, _)) if !d.is_empty() => d.to_string(),
            Some(_) => "/".to_string(),
            None => ".".to_string(),
        };
        let tmp = format!("{final_path}.laika-part");
        format!(
            "set -e; d={d}; t={t}; f={f}; mkdir -p \"$d\"; cat > \"$t\"; mv -f \"$t\" \"$f\"; \
             if command -v sha256sum >/dev/null 2>&1; then sha256sum \"$f\"; \
             else shasum -a 256 \"$f\"; fi",
            d = quote(&dir),
            t = quote(&tmp),
            f = quote(final_path),
        )
    }

    /// Shell script run on the server for one download. Redirection avoids
    /// treating a path beginning with `-` as a `cat` option.
    pub fn download_script(final_path: &str) -> String {
        format!(
            "set -e; f={}; test -f \"$f\"; cat < \"$f\"",
            quote(final_path)
        )
    }

    fn ssh_args(s: &SyncSettings) -> Vec<String> {
        let mut args: Vec<String> = [
            "-o",
            "BatchMode=yes",
            "-o",
            "ConnectTimeout=10",
            "-o",
            "ServerAliveInterval=15",
            "-o",
            "StrictHostKeyChecking=accept-new",
        ]
        .iter()
        .map(|a| a.to_string())
        .collect();
        // Windows OpenSSH does not support Unix-domain control sockets.
        #[cfg(unix)]
        args.extend(
            [
                "-o",
                "ControlMaster=auto",
                "-o",
                "ControlPath=/tmp/laika-ssh-%C",
                "-o",
                "ControlPersist=120",
            ]
            .iter()
            .map(|a| a.to_string()),
        );
        let port = s.sftp_port.trim();
        if !port.is_empty() {
            args.push("-p".into());
            args.push(port.to_string());
        }
        let identity = s.sftp_identity.trim();
        if !identity.is_empty() {
            let expanded = match identity.strip_prefix("~/") {
                Some(rest) => crate::platform::home_dir()
                    .join(rest)
                    .to_string_lossy()
                    .to_string(),
                None => identity.to_string(),
            };
            args.push("-i".into());
            args.push(expanded);
        }
        let dest = if s.sftp_user.trim().is_empty() {
            s.sftp_host.trim().to_string()
        } else {
            format!("{}@{}", s.sftp_user.trim(), s.sftp_host.trim())
        };
        args.push(dest);
        args
    }

    fn last_line(bytes: &[u8]) -> String {
        String::from_utf8_lossy(bytes)
            .lines()
            .map(str::trim)
            .filter(|l| !l.is_empty())
            .last()
            .unwrap_or("")
            .to_string()
    }

    /// Run `script` remotely, streaming `input` to its stdin (sha256 of
    /// the streamed bytes returned alongside stdout).
    fn run(
        s: &SyncSettings,
        script: &str,
        input: Option<&Path>,
    ) -> Result<(String, String, u64), String> {
        let mut cmd = Command::new("ssh");
        cmd.args(ssh_args(s))
            .arg(script)
            .stdin(if input.is_some() {
                Stdio::piped()
            } else {
                Stdio::null()
            })
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let mut child = cmd.spawn().map_err(|e| format!("start ssh: {e}"))?;
        let mut sent = 0u64;
        let mut hasher = Sha256::new();
        if let Some(path) = input {
            let mut stdin = child.stdin.take().expect("piped stdin");
            let mut file =
                std::fs::File::open(path).map_err(|e| format!("read {}: {e}", path.display()))?;
            let mut buf = vec![0u8; 1 << 20];
            loop {
                let n = file
                    .read(&mut buf)
                    .map_err(|e| format!("read {}: {e}", path.display()))?;
                if n == 0 {
                    break;
                }
                hasher.update(&buf[..n]);
                if stdin.write_all(&buf[..n]).is_err() {
                    // Remote side closed early: the exit status explains why.
                    break;
                }
                sent += n as u64;
            }
            drop(stdin);
        }
        let out = child.wait_with_output().map_err(|e| format!("ssh: {e}"))?;
        if !out.status.success() {
            let err = last_line(&out.stderr);
            return Err(if err.is_empty() {
                format!("ssh exited with {}", out.status)
            } else {
                err
            });
        }
        let digest = hasher.finalize();
        let hex = digest
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<String>();
        Ok((String::from_utf8_lossy(&out.stdout).to_string(), hex, sent))
    }

    pub fn upload(s: &SyncSettings, local: &Path, key: &str) -> Result<u64, String> {
        let remote = remote_path(&s.sftp_path, key);
        let (stdout, local_sha, sent) = run(s, &upload_script(&remote), Some(local))?;
        let remote_sha = stdout
            .split_whitespace()
            .next()
            .unwrap_or("")
            .to_lowercase();
        if remote_sha != local_sha {
            return Err(format!(
                "verify {remote}: remote sha256 {remote_sha:?} != local {local_sha}"
            ));
        }
        Ok(sent)
    }

    fn install_without_overwrite(tmp: &Path, destination: &Path) -> Result<(), String> {
        if destination.exists() {
            return Err(format!(
                "refusing to overwrite existing {}",
                destination.display()
            ));
        }
        // A same-filesystem hard link is atomic and fails if the destination
        // appeared during the transfer. Some network/removable filesystems do
        // not support hard links, so fall back to create_new + verified copy.
        match std::fs::hard_link(tmp, destination) {
            Ok(()) => {
                std::fs::remove_file(tmp).ok();
                Ok(())
            }
            Err(link_error) => {
                if destination.exists() {
                    return Err(format!(
                        "refusing to overwrite existing {}",
                        destination.display()
                    ));
                }
                let copied = (|| -> Result<(), String> {
                    let mut src = std::fs::File::open(tmp)
                        .map_err(|e| format!("read {}: {e}", tmp.display()))?;
                    let mut out = std::fs::OpenOptions::new()
                        .write(true)
                        .create_new(true)
                        .open(destination)
                        .map_err(|e| {
                            format!(
                                "create restored {}: {e} (hard-link fallback: {link_error})",
                                destination.display()
                            )
                        })?;
                    std::io::copy(&mut src, &mut out)
                        .map_err(|e| format!("restore {}: {e}", destination.display()))?;
                    out.sync_all()
                        .map_err(|e| format!("flush {}: {e}", destination.display()))
                })();
                if let Err(e) = copied {
                    std::fs::remove_file(destination).ok();
                    return Err(e);
                }
                std::fs::remove_file(tmp).ok();
                Ok(())
            }
        }
    }

    /// Stream one remote backup into a sibling temporary file, verify the
    /// catalog's import-time BLAKE3, then install without replacing anything.
    pub fn download(
        s: &SyncSettings,
        key: &str,
        destination: &Path,
        expected_blake3: &str,
    ) -> Result<u64, String> {
        if expected_blake3.trim().is_empty() {
            return Err("catalog has no checksum for this original".to_string());
        }
        if destination.exists() {
            return Err(format!(
                "refusing to overwrite existing {}",
                destination.display()
            ));
        }
        let parent = destination
            .parent()
            .ok_or_else(|| format!("{} has no parent folder", destination.display()))?;
        std::fs::create_dir_all(parent).map_err(|e| format!("create {}: {e}", parent.display()))?;
        let name = destination
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("original");
        let tmp = parent.join(format!(".{name}.laika-restore-{}.part", std::process::id()));
        std::fs::remove_file(&tmp).ok();
        let mut out = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&tmp)
            .map_err(|e| format!("create {}: {e}", tmp.display()))?;

        let remote = remote_path(&s.sftp_path, key);
        let mut cmd = Command::new("ssh");
        cmd.args(ssh_args(s))
            .arg(download_script(&remote))
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let mut child = match cmd.spawn() {
            Ok(child) => child,
            Err(e) => {
                std::fs::remove_file(&tmp).ok();
                return Err(format!("start ssh: {e}"));
            }
        };
        let mut stdout = child.stdout.take().expect("piped stdout");
        let mut hasher = blake3::Hasher::new();
        let mut bytes = 0u64;
        let mut buf = vec![0u8; 1 << 20];
        let streamed = (|| -> Result<(), String> {
            loop {
                let n = stdout
                    .read(&mut buf)
                    .map_err(|e| format!("download {remote}: {e}"))?;
                if n == 0 {
                    break;
                }
                hasher.update(&buf[..n]);
                out.write_all(&buf[..n])
                    .map_err(|e| format!("write {}: {e}", tmp.display()))?;
                bytes += n as u64;
            }
            out.sync_all()
                .map_err(|e| format!("flush {}: {e}", tmp.display()))
        })();
        drop(stdout);
        drop(out);
        if let Err(e) = streamed {
            child.kill().ok();
            child.wait().ok();
            std::fs::remove_file(&tmp).ok();
            return Err(e);
        }
        let output = child.wait_with_output().map_err(|e| {
            std::fs::remove_file(&tmp).ok();
            format!("ssh: {e}")
        })?;
        if !output.status.success() {
            std::fs::remove_file(&tmp).ok();
            let error = last_line(&output.stderr);
            return Err(if error.is_empty() {
                format!("download {remote}: ssh exited with {}", output.status)
            } else {
                format!("download {remote}: {error}")
            });
        }
        let got = hasher.finalize().to_hex().to_string();
        if !got.eq_ignore_ascii_case(expected_blake3.trim()) {
            std::fs::remove_file(&tmp).ok();
            return Err(format!(
                "verify {remote}: downloaded blake3 {got} != catalog {}",
                expected_blake3.trim()
            ));
        }
        if let Err(e) = install_without_overwrite(&tmp, destination) {
            std::fs::remove_file(&tmp).ok();
            return Err(e);
        }
        Ok(bytes)
    }

    /// Log in, create the base folder, and confirm it's writable.
    pub fn check(s: &SyncSettings) -> Result<(), String> {
        let base = remote_path(&s.sftp_path, "");
        let base = if base.is_empty() {
            ".".to_string()
        } else {
            base
        };
        let script = format!(
            "mkdir -p {b} && test -w {b} && echo laika-ok",
            b = quote(base.trim_end_matches('/'))
        );
        let (stdout, _, _) = run(s, &script, None)?;
        if stdout.contains("laika-ok") {
            Ok(())
        } else {
            Err(format!("{} is not writable", s.sftp_path))
        }
    }

    #[cfg(test)]
    mod restore_tests {
        use super::{install_without_overwrite, ssh_args};
        use crate::sync::SyncSettings;

        #[test]
        fn ssh_options_always_have_values() {
            let settings = SyncSettings {
                sftp_host: "orion.local".to_string(),
                sftp_port: "22".to_string(),
                ..SyncSettings::default()
            };
            let args = ssh_args(&settings);
            for (index, arg) in args.iter().enumerate() {
                if arg == "-o" || arg == "-p" || arg == "-i" {
                    assert!(
                        args.get(index + 1).is_some_and(|value| !value.is_empty()),
                        "SSH option {arg} at {index} has no value: {args:?}"
                    );
                }
            }
            assert_eq!(args.last().map(String::as_str), Some("orion.local"));
        }

        #[test]
        fn verified_temp_install_never_replaces_a_file() {
            let root =
                std::env::temp_dir().join(format!("laika-sftp-install-{}", std::process::id()));
            std::fs::create_dir_all(&root).unwrap();
            let destination = root.join("photo.nef");
            let temp = root.join(".photo.part");
            std::fs::write(&destination, b"existing").unwrap();
            std::fs::write(&temp, b"restored").unwrap();
            assert!(
                install_without_overwrite(&temp, &destination)
                    .unwrap_err()
                    .contains("refusing to overwrite")
            );
            assert_eq!(std::fs::read(&destination).unwrap(), b"existing");

            std::fs::remove_file(&destination).unwrap();
            install_without_overwrite(&temp, &destination).unwrap();
            assert_eq!(std::fs::read(&destination).unwrap(), b"restored");
            assert!(!temp.exists());
            std::fs::remove_dir_all(&root).ok();
        }
    }
}

/// Backup into a mounted SMB/NFS share: copy to a temp name while hashing,
/// fsync, rename into place, then re-read the destination and compare
/// blake3 so a flaky share can't report success for torn bytes.
pub mod share {
    use super::SyncSettings;
    use std::io::{Read, Write};
    use std::path::{Path, PathBuf};

    fn base(s: &SyncSettings) -> Result<PathBuf, String> {
        let base = PathBuf::from(s.share_path.trim());
        if !base.is_dir() {
            return Err(format!(
                "{} is not available — connect the share first",
                base.display()
            ));
        }
        Ok(base)
    }

    fn blake3_file(path: &Path) -> Result<String, String> {
        let mut hasher = blake3::Hasher::new();
        let mut f =
            std::fs::File::open(path).map_err(|e| format!("read {}: {e}", path.display()))?;
        std::io::copy(&mut f, &mut hasher).map_err(|e| format!("read {}: {e}", path.display()))?;
        Ok(hasher.finalize().to_hex().to_string())
    }

    pub fn upload(s: &SyncSettings, local: &Path, key: &str) -> Result<u64, String> {
        let dest = base(s)?.join(key.trim_start_matches('/'));
        let dir = dest.parent().ok_or("bad destination")?;
        std::fs::create_dir_all(dir).map_err(|e| format!("mkdir {}: {e}", dir.display()))?;
        let name = dest
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("file")
            .to_string();
        let tmp = dir.join(format!(".{name}.laika-part"));
        let mut src =
            std::fs::File::open(local).map_err(|e| format!("read {}: {e}", local.display()))?;
        let mut out =
            std::fs::File::create(&tmp).map_err(|e| format!("write {}: {e}", tmp.display()))?;
        let mut hasher = blake3::Hasher::new();
        let mut buf = vec![0u8; 1 << 20];
        let mut sent = 0u64;
        let copied = (|| -> Result<(), String> {
            loop {
                let n = src
                    .read(&mut buf)
                    .map_err(|e| format!("read {}: {e}", local.display()))?;
                if n == 0 {
                    break;
                }
                hasher.update(&buf[..n]);
                out.write_all(&buf[..n])
                    .map_err(|e| format!("write {}: {e}", tmp.display()))?;
                sent += n as u64;
            }
            out.sync_all()
                .map_err(|e| format!("flush {}: {e}", tmp.display()))
        })();
        drop(out);
        if let Err(e) = copied {
            std::fs::remove_file(&tmp).ok();
            return Err(e);
        }
        std::fs::rename(&tmp, &dest).map_err(|e| {
            std::fs::remove_file(&tmp).ok();
            format!("rename into {}: {e}", dest.display())
        })?;
        let local_hash = hasher.finalize().to_hex().to_string();
        let stored = blake3_file(&dest)?;
        if stored != local_hash {
            return Err(format!(
                "verify {}: stored blake3 {stored} != local {local_hash}",
                dest.display()
            ));
        }
        Ok(sent)
    }

    /// The folder exists and a probe file can be written and removed.
    pub fn check(s: &SyncSettings) -> Result<(), String> {
        let base = base(s)?;
        let probe = base.join(format!(".laika-write-test-{}", std::process::id()));
        std::fs::write(&probe, b"laika")
            .map_err(|e| format!("{} is not writable: {e}", base.display()))?;
        std::fs::remove_file(&probe).ok();
        Ok(())
    }
}

/// Photo id + what to upload. Resolved to local path + hash at claim time.
#[derive(Clone, Debug)]
pub struct SyncJob {
    pub rowid: i64,
    pub photo_id: i64,
    pub kind: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sftp_paths_and_quoting() {
        assert_eq!(sftp::quote("a'b"), "'a'\\''b'");
        assert_eq!(
            sftp::remote_path("~/backup/", "cat/2026/x.nef"),
            "backup/cat/2026/x.nef"
        );
        assert_eq!(
            sftp::remote_path("/srv/photos", "/cat/x"),
            "/srv/photos/cat/x"
        );
        assert_eq!(sftp::remote_path("~", "cat/x"), "cat/x");
    }

    /// The remote script runs under a local `sh` exactly as ssh would run
    /// it: creates folders, lands the file atomically, prints its sha256.
    #[cfg(unix)]
    #[test]
    fn sftp_upload_script_runs_locally() {
        use std::io::Write;
        let dir = std::env::temp_dir().join(format!("laika-sftp-script-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let fin = dir.join("deep dir/it's/photo.nef");
        let script = sftp::upload_script(fin.to_str().unwrap());
        let mut child = std::process::Command::new("sh")
            .arg("-c")
            .arg(&script)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .spawn()
            .unwrap();
        child.stdin.take().unwrap().write_all(b"raw-bytes").unwrap();
        let out = child.wait_with_output().unwrap();
        assert!(out.status.success(), "{:?}", out);
        assert_eq!(std::fs::read(&fin).unwrap(), b"raw-bytes");
        assert!(!dir.join("deep dir/it's/photo.nef.laika-part").exists());
        let printed = String::from_utf8_lossy(&out.stdout);
        use sha2::Digest;
        let want: String = sha2::Sha256::digest(b"raw-bytes")
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();
        assert!(printed.starts_with(&want), "{printed}");
        let downloaded = std::process::Command::new("sh")
            .arg("-c")
            .arg(sftp::download_script(fin.to_str().unwrap()))
            .output()
            .unwrap();
        assert!(downloaded.status.success());
        assert_eq!(downloaded.stdout, b"raw-bytes");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn share_upload_verifies_and_is_atomic() {
        let root = std::env::temp_dir().join(format!("laika-share-{}", std::process::id()));
        let share = root.join("mnt");
        std::fs::create_dir_all(&share).unwrap();
        let local = root.join("src.nef");
        std::fs::write(&local, vec![7u8; 3_000_000]).unwrap();
        let s = SyncSettings {
            target: BackupTarget::Share,
            share_path: share.to_string_lossy().into(),
            ..Default::default()
        };
        assert!(s.configured());
        check_blocking(&s, "").expect("writable share");
        let key = remote_key("Weddings", "2026:06:14 18:42:00", &local, false);
        let n = upload_blocking(&s, "", &local, &key, "unused").unwrap();
        assert_eq!(n, 3_000_000);
        let dest = share.join("weddings/2026/2026-06-14/src.nef");
        assert_eq!(std::fs::read(&dest).unwrap().len(), 3_000_000);
        assert!(!dest.with_file_name(".src.nef.laika-part").exists());
        // A missing mount is a clear error, not a silent local copy.
        let gone = SyncSettings {
            share_path: root.join("not-mounted").to_string_lossy().into(),
            ..s.clone()
        };
        assert!(upload_blocking(&gone, "", &local, &key, "").is_err());
        std::fs::remove_dir_all(&root).ok();
    }

    /// Live SFTP roundtrip. Runs only with LAIKA_SFTP_TEST=host (key auth
    /// must already work non-interactively; writes under the temp dir).
    #[test]
    fn sftp_live_roundtrip() {
        let Ok(host) = std::env::var("LAIKA_SFTP_TEST") else {
            return;
        };
        let dir = std::env::temp_dir().join(format!("laika-sftp-live-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let local = dir.join("payload.nef");
        std::fs::write(&local, vec![42u8; 5_000_000]).unwrap();
        let remote_base = dir.join("remote");
        let s = SyncSettings {
            target: BackupTarget::Sftp,
            sftp_host: host,
            sftp_path: remote_base.to_string_lossy().into(),
            ..Default::default()
        };
        check_blocking(&s, "").expect("ssh check");
        let n = upload_blocking(&s, "", &local, "cat/2026/2026-01-01/payload.nef", "").unwrap();
        assert_eq!(n, 5_000_000);
        let landed = remote_base.join("cat/2026/2026-01-01/payload.nef");
        assert_eq!(std::fs::read(&landed).unwrap().len(), 5_000_000);
        let restored = dir.join("restored/payload.nef");
        let hash = blake3::hash(&std::fs::read(&local).unwrap())
            .to_hex()
            .to_string();
        let n = restore_blocking(&s, "cat/2026/2026-01-01/payload.nef", &restored, &hash).unwrap();
        assert_eq!(n, 5_000_000);
        assert_eq!(
            std::fs::read(&restored).unwrap(),
            std::fs::read(&local).unwrap()
        );
        assert!(
            restore_blocking(&s, "cat/2026/2026-01-01/payload.nef", &restored, &hash,)
                .unwrap_err()
                .contains("refusing to overwrite")
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn target_defaults_and_urls() {
        let mut s = SyncSettings::default();
        assert_eq!(s.target, BackupTarget::Sftp);
        assert!(!s.configured());
        s.share_server = "nas.local".into();
        s.share_name = "photos".into();
        assert_eq!(s.share_url().as_deref(), Some("smb://nas.local/photos"));
        s.share_kind = ShareKind::Nfs;
        s.share_name = "/srv/photos".into();
        assert_eq!(s.share_url().as_deref(), Some("nfs://nas.local/srv/photos"));
        assert_eq!(
            s.share_mount_guess().unwrap(),
            std::path::PathBuf::from("/Volumes/photos")
        );
    }

    #[test]
    fn key_layout() {
        let p = Path::new("/arc/2026/shoot/DSC_1.nef");
        assert_eq!(
            remote_key("weddings", "2026:06:14 18:42:00", p, false),
            "weddings/2026/2026-06-14/DSC_1.nef"
        );
        assert_eq!(
            remote_key("weddings", "2026:06:14 18:42:00", p, true),
            "weddings/2026/2026-06-14/DSC_1.nef.xmp"
        );
        assert_eq!(
            remote_key("weddings", "", Path::new("x.jpg"), false)
                .split('/')
                .count(),
            4
        );
    }

    #[test]
    fn versioned_keys_are_idempotent_and_do_not_collide() {
        let p = Path::new("IMG_0001.NEF");
        let a = versioned_remote_key(
            "Wedding",
            "2026:06:14 18:42:00",
            p,
            false,
            "aabbccddeeff001122",
        );
        let b = versioned_remote_key(
            "Wedding",
            "2026:06:14 18:42:00",
            p,
            false,
            "112233445566778899",
        );
        assert_eq!(a, "wedding/2026/2026-06-14/aabbccddeeff001122-IMG_0001.NEF");
        assert_ne!(a, b);
        assert_eq!(
            versioned_remote_key(
                "Wedding",
                "2026:06:14 18:42:00",
                p,
                true,
                "aabbccddeeff001122",
            ),
            "wedding/2026/2026-06-14/aabbccddeeff001122-IMG_0001.NEF.xmp"
        );
    }

    #[test]
    fn date_prefix_parses_exif() {
        let (y, ymd) = date_prefix("2026:06:14 18:42:00", Path::new("/none"));
        assert_eq!((y.as_str(), ymd.as_str()), ("2026", "2026-06-14"));
        let (y, _) = date_prefix("garbage", Path::new("/nonexistent-xyz"));
        assert_eq!(y, "unknown");
        // Multi-byte char straddling byte 10 must not panic.
        let (y, _) = date_prefix("2026:06:1é 18:42:00", Path::new("/nonexistent-xyz"));
        assert_eq!(y, "unknown");
        let (y, _) = date_prefix("2026--0614 18:42:00", Path::new("/nonexistent-xyz"));
        assert_eq!(y, "unknown");
    }

    #[test]
    fn state_names() {
        // SyncState round-trips through the strings stored in SQLite.
        for (s, name) in [
            (SyncState::Local, "local"),
            (SyncState::Pending, "pending"),
            (SyncState::Synced, "synced"),
            (SyncState::Failed, "failed"),
        ] {
            let back = match name {
                "pending" => SyncState::Pending,
                "synced" => SyncState::Synced,
                "failed" => SyncState::Failed,
                _ => SyncState::Local,
            };
            assert_eq!(s, back);
        }
    }

    /// Live MinIO/S3 roundtrip. Runs only with LAIKA_SYNC_TEST=1 and
    /// LAIKA_S3_* env set (local MinIO: endpoint http://localhost:9000,
    /// bucket must already exist — create it once with e.g.
    /// `aws --endpoint-url http://localhost:9000 s3 mb s3://<bucket>`).
    #[tokio::test]
    async fn minio_roundtrip() {
        if std::env::var("LAIKA_SYNC_TEST").as_deref() != Ok("1") {
            return;
        }
        let settings = SyncSettings {
            endpoint: std::env::var("LAIKA_S3_ENDPOINT").unwrap(),
            bucket: std::env::var("LAIKA_S3_BUCKET").unwrap(),
            region: std::env::var("LAIKA_S3_REGION").unwrap_or("us-east-1".into()),
            access_key: std::env::var("LAIKA_S3_KEY").unwrap(),
            target: BackupTarget::S3,
            ..Default::default()
        };
        let secret = std::env::var("LAIKA_S3_SECRET").unwrap();
        let store = build_store(&settings, &secret).expect("store builds");
        let dir = std::env::temp_dir().join(format!("laika-minio-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let local = dir.join("payload.bin");
        std::fs::write(&local, b"minio-verify-me").unwrap();
        let n = upload_and_verify(&store, &local, "laika-test/verify.bin", "deadbeef")
            .await
            .unwrap();
        assert_eq!(n, 15);
        // Verifying against the wrong hash must fail (object stays, state doesn't).
        let bad = verify_remote(&store, "laika-test/verify.bin", "wrong").await;
        assert!(bad.is_err(), "hash mismatch must fail, got {bad:?}");
        std::fs::remove_dir_all(&dir).ok();
    }

    /// In-memory roundtrip of the put + metadata-verify chain (no network).
    #[tokio::test]
    async fn memory_roundtrip() {
        use object_store::memory::InMemory;
        let store = InMemory::new();
        let dir = std::env::temp_dir().join(format!("laika-sync-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let local = dir.join("f.bin");
        std::fs::write(&local, b"hello-sync").unwrap();
        let bytes = upload_and_verify_generic(&store, &local, "weddings/2026/f.bin", "abc123")
            .await
            .unwrap();
        assert_eq!(bytes, 10);
        std::fs::remove_dir_all(&dir).ok();
    }

    async fn upload_and_verify_generic(
        store: &impl ObjectStore,
        local: &Path,
        remote: &str,
        hash: &str,
    ) -> Result<u64, String> {
        let bytes = std::fs::read(local).map_err(|e| e.to_string())?;
        let len = bytes.len() as u64;
        let path = StorePath::from(remote);
        store
            .put_opts(
                &path,
                PutPayload::from_bytes(bytes.into()),
                put_options(hash),
            )
            .await
            .map_err(|e| e.to_string())?;
        let opts = GetOptions {
            range: Some(GetRange::Bounded(0..1)),
            ..Default::default()
        };
        let got = store
            .get_opts(&path, opts)
            .await
            .map_err(|e| e.to_string())?;
        let stored = got
            .attributes
            .get(&Attribute::Metadata(BLAKE_ATTR.into()))
            .map(|s| s.to_string())
            .unwrap_or_default();
        assert_eq!(stored, hash);
        Ok(len)
    }
}
