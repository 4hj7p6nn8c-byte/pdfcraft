//! Staging files for atomic saves: a new, unpredictably named file beside the target, written and
//! then renamed over it. Writing, closing, renaming and cleaning up stay with the caller.

use std::path::{Path, PathBuf};

/// Reject replacing the source (including symlink aliases) and require explicit overwrite
/// authorization. Call both before expensive work and immediately before publishing output.
pub fn check_copy_destination(target: &Path, source: Option<&Path>, overwrite: bool) -> std::io::Result<()> {
    let bad = |s| std::io::Error::new(std::io::ErrorKind::AlreadyExists, s);
    if target.is_dir() {
        return Err(bad("The output is a folder, not a PDF file"));
    }
    if let Some(source) = source {
        let absolute = |p: &Path| -> std::io::Result<PathBuf> {
            if p.exists() {
                p.canonicalize()
            } else {
                let parent = p.parent().filter(|p| !p.as_os_str().is_empty()).unwrap_or(Path::new("."));
                let name = p.file_name().ok_or_else(|| bad("Invalid output filename"))?;
                Ok(parent.canonicalize()?.join(name))
            }
        };
        if absolute(target)? == absolute(source)? {
            return Err(bad("A searchable copy cannot replace its source; choose a different filename"));
        }
    }
    if target.symlink_metadata().is_ok() && !overwrite {
        return Err(bad("The output already exists; choose a new filename or explicitly approve overwrite"));
    }
    Ok(())
}

/// Atomic publication of a separate copy. Hard-linking the closed staging file publishes
/// without replacement even if another file appears during the job. A cancelled/failed write
/// removes its staging file. Rename replaces an existing target only after explicit approval.
pub fn write_copy_atomic(
    target: &Path,
    source: Option<&Path>,
    bytes: &[u8],
    overwrite: bool,
    mut keep_going: impl FnMut() -> bool,
) -> std::io::Result<()> {
    use std::io::Write;
    check_copy_destination(target, source, overwrite)?;
    let parent = target.parent().filter(|p| !p.as_os_str().is_empty()).unwrap_or(Path::new("."));
    let name = target.file_name().ok_or_else(|| std::io::Error::new(std::io::ErrorKind::InvalidInput, "Invalid output filename"))?;
    let (tmp, mut file) = create_staging(parent, &name.to_string_lossy(), StagingName::SuffixThenTag, staging_suffixes())?;
    let result = (|| {
        for chunk in bytes.chunks(1024 * 1024) {
            if !keep_going() {
                return Err(std::io::Error::new(std::io::ErrorKind::Interrupted, "OCR cancelled; no copy was saved"));
            }
            file.write_all(chunk)?;
        }
        file.sync_all()?;
        drop(file);
        check_copy_destination(target, source, overwrite)?;
        if !keep_going() {
            return Err(std::io::Error::new(std::io::ErrorKind::Interrupted, "OCR cancelled; no copy was saved"));
        }
        if overwrite { std::fs::rename(&tmp, target) } else { std::fs::hard_link(&tmp, target) }
    })();
    let _ = std::fs::remove_file(tmp);
    result
}

/// How many staging names [`create_staging`] tries. A random 64-bit name is only taken if someone
/// put a file there on purpose, so running out means refusing, not trying harder.
pub const STAGING_ATTEMPTS: usize = 16;

/// Where the suffix goes in a staging name. Each caller keeps the form it has always used.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StagingName {
    /// `.{name}.{suffix}.pdfcraft-tmp`, used by the automation tools.
    SuffixThenTag,
    /// `.{name}.pdfcraft-{suffix}.tmp`, used by the desktop app's save.
    TagThenSuffix,
}

/// Create a new, empty staging file in `dir` for the file `name`, one name per suffix. It is
/// opened with `create_new`, which fails if anything already has the name (a file, a hard link,
/// a symbolic link even when dangling, a folder), on Windows as everywhere else; such a name is
/// skipped, never opened, so a file planted at the staging path can't receive or redirect the
/// write. Pass [`staging_suffixes`]: fixed suffixes are for tests, and a predictable name can be
/// taken in advance to refuse every save.
pub fn create_staging(
    dir: &Path,
    name: &str,
    form: StagingName,
    suffixes: impl IntoIterator<Item = u64>,
) -> std::io::Result<(PathBuf, std::fs::File)> {
    // At most 128 bytes of the target's name, cut between characters, so the staging name fits
    // the 255-byte (Linux, macOS) and 255-unit (Windows) limits however long that name is.
    let mut stem = String::new();
    for c in name.chars() {
        if stem.len() + c.len_utf8() > 128 {
            break;
        }
        stem.push(c);
    }
    for suffix in suffixes.into_iter().take(STAGING_ATTEMPTS) {
        let tmp = dir.join(match form {
            StagingName::SuffixThenTag => format!(".{stem}.{suffix:016x}.pdfcraft-tmp"),
            StagingName::TagThenSuffix => format!(".{stem}.pdfcraft-{suffix:016x}.tmp"),
        });
        match std::fs::OpenOptions::new().write(true).create_new(true).open(&tmp) {
            Ok(file) => return Ok((tmp, file)),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
            // Windows reports a folder at the name as "access denied"; it is taken all the same.
            Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied && tmp.symlink_metadata().is_ok() => {}
            Err(e) => return Err(e),
        }
    }
    Err(std::io::Error::new(std::io::ErrorKind::AlreadyExists, "every temporary file name tried is taken"))
}

/// Unpredictable staging-name suffixes, so a name can't be planted in advance. Each is the
/// standard library's keyed hash of a counter (`RandomState::hash_one`, currently SipHash-1-3),
/// keyed from the operating system's random source. That is all a staging name needs, but it is
/// not a CSPRNG: don't use these where a value must stay secret or unguessable after it has been
/// seen, such as a token or a key.
pub fn staging_suffixes() -> impl Iterator<Item = u64> {
    use std::hash::BuildHasher;
    let state = std::hash::RandomState::new();
    (0u64..).map(move |i| state.hash_one(i))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn searchable_copy_collision_source_cancel_and_write_failure_are_safe() {
        let dir = test_dir("copy-safe");
        let source = dir.join("source.pdf");
        let target = dir.join("copy.pdf");
        std::fs::write(&source, b"ORIGINAL").unwrap();
        assert!(write_copy_atomic(&source, Some(&source), b"NEW", true, || true).is_err());
        assert!(write_copy_atomic(&target, Some(&source), b"NEW", false, || false).is_err());
        assert!(!target.exists());
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 1);
        write_copy_atomic(&target, Some(&source), b"NEW", false, || true).unwrap();
        assert!(write_copy_atomic(&target, Some(&source), b"BAD", false, || true).is_err());
        assert_eq!(std::fs::read(&target).unwrap(), b"NEW");
        write_copy_atomic(&target, Some(&source), b"APPROVED", true, || true).unwrap();
        assert_eq!(std::fs::read(&target).unwrap(), b"APPROVED");
        assert!(write_copy_atomic(&dir.join("missing").join("out.pdf"), Some(&source), b"BAD", false, || true).is_err());
        assert_eq!(std::fs::read(&source).unwrap(), b"ORIGINAL");
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 2, "no staging files remain");
    }

    #[test]
    fn searchable_copy_racing_collision_never_replaces_an_existing_file() {
        let dir = test_dir("copy-race");
        let target = dir.join("copy.pdf");
        let mut calls = 0;
        let r = write_copy_atomic(&target, None, b"NEW", false, || {
            calls += 1;
            if calls == 2 {
                std::fs::write(&target, b"EXISTING").unwrap();
            }
            true
        });
        assert!(r.is_err());
        assert_eq!(std::fs::read(&target).unwrap(), b"EXISTING");
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 1);
    }

    #[test]
    fn searchable_copy_cancel_during_write_cleans_staging_and_keeps_existing_output() {
        let dir = test_dir("copy-cancel-writing");
        let target = dir.join("copy.pdf");
        std::fs::write(&target, b"EXISTING").unwrap();
        let mut calls = 0;
        let bytes = vec![42; 2 * 1024 * 1024];
        let result = write_copy_atomic(&target, None, &bytes, true, || {
            calls += 1;
            calls < 2
        });
        assert_eq!(result.unwrap_err().kind(), std::io::ErrorKind::Interrupted);
        assert_eq!(std::fs::read(&target).unwrap(), b"EXISTING");
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 1);
    }

    /// A fresh, empty folder for one test.
    fn test_dir(test: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("pdfcraft-platform-staging-{}-{test}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn file_name(p: &Path) -> String {
        p.file_name().unwrap().to_string_lossy().into_owned()
    }

    /// The callers' tests plant files at these exact names; if a name changed, those tests would
    /// plant where nothing writes and pass without testing anything.
    #[test]
    fn each_form_keeps_its_exact_name() {
        let dir = test_dir("forms");
        let (tools, _) = create_staging(&dir, "out.pdf", StagingName::SuffixThenTag, [0xab]).unwrap();
        assert_eq!(file_name(&tools), ".out.pdf.00000000000000ab.pdfcraft-tmp");
        let (desktop, _) = create_staging(&dir, "out.pdf", StagingName::TagThenSuffix, [0xab]).unwrap();
        assert_eq!(file_name(&desktop), ".out.pdf.pdfcraft-00000000000000ab.tmp");
        assert_eq!(std::fs::read(&tools).unwrap(), b"", "a new, empty file");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_long_name_is_cut_to_128_bytes_between_characters() {
        let dir = test_dir("long");
        // One byte, then four-byte characters: the 128-byte limit falls inside the 32nd one.
        let name = format!("a{}.pdf", "\u{1F600}".repeat(40));
        let (tmp, _) = create_staging(&dir, &name, StagingName::SuffixThenTag, [1]).unwrap();
        assert_eq!(file_name(&tmp), format!(".a{}.0000000000000001.pdfcraft-tmp", "\u{1F600}".repeat(31)));
        // One-byte characters: exactly 128 of them are kept.
        let (tmp, _) = create_staging(&dir, &"x".repeat(200), StagingName::SuffixThenTag, [2]).unwrap();
        assert_eq!(file_name(&tmp), format!(".{}.0000000000000002.pdfcraft-tmp", "x".repeat(128)));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn running_out_of_suffixes_refuses_and_creates_nothing() {
        let dir = test_dir("empty");
        let e = create_staging(&dir, "out.pdf", StagingName::TagThenSuffix, []).unwrap_err();
        assert_eq!(e.kind(), std::io::ErrorKind::AlreadyExists);
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 0);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn only_the_first_sixteen_names_are_tried() {
        let dir = test_dir("cap");
        let name = |s: u64| dir.join(format!(".out.pdf.{s:016x}.pdfcraft-tmp"));
        for s in 1..=STAGING_ATTEMPTS as u64 {
            std::fs::write(name(s), "PLANTED").unwrap();
        }
        assert!(create_staging(&dir, "out.pdf", StagingName::SuffixThenTag, 1..).is_err());
        assert!(!name(STAGING_ATTEMPTS as u64 + 1).exists(), "a 17th name was tried");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
