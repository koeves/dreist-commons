//! Saving user files.

use std::io::Write;
use std::path::Path;

/// Write `bytes` to `path` all or nothing: into a temporary file next to it,
/// flushed to disk, then renamed over it. A crash or a full disk leaves the
/// previous file as it was.
pub fn write_atomic(path: impl AsRef<Path>, bytes: impl AsRef<[u8]>) -> std::io::Result<()> {
    let path = path.as_ref();
    let name = path.file_name().ok_or_else(|| std::io::Error::new(std::io::ErrorKind::InvalidInput, "no file name"))?;
    let mut tmp_name = std::ffi::OsString::from(".");
    tmp_name.push(name);
    tmp_name.push(format!(".{}.tmp", pid()));
    let tmp = path.with_file_name(tmp_name);
    let write = || -> std::io::Result<()> {
        let mut f = std::fs::File::create(&tmp)?;
        f.write_all(bytes.as_ref())?;
        f.sync_all()
    };
    if let Err(e) = write().and_then(|()| std::fs::rename(&tmp, path)) {
        let _ = std::fs::remove_file(&tmp);
        return Err(e);
    }
    Ok(())
}

/// This process's id.
fn pid() -> u32 {
    std::process::id()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replaces_whole_or_not_at_all() {
        let dir = std::env::temp_dir().join(format!("dreist-atomic-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("a.geist");
        write_atomic(&p, "one").unwrap();
        write_atomic(&p, "two").unwrap();
        assert_eq!(std::fs::read_to_string(&p).unwrap(), "two");
        // A failed write (the target is a directory) keeps nothing behind.
        let d = dir.join("sub");
        std::fs::create_dir_all(&d).unwrap();
        assert!(write_atomic(&d, "x").is_err());
        let left: Vec<_> = std::fs::read_dir(&dir).unwrap().flatten().map(|e| e.file_name()).collect();
        assert_eq!(left.len(), 2, "{left:?}");
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
