//! Snapshot local inputs once. The verifier and store consume those same bytes.

use std::fs::File;
use std::io::{self, Read};
use std::path::Path;

use super::{PROFILE_LIMIT, VerificationError};

#[cfg(unix)]
fn open_regular(path: &Path) -> io::Result<File> {
    use std::path::Component;

    use rustix::fs::{Mode, OFlags, open, openat};

    // Bind each parent directory to an open descriptor. NOFOLLOW on the leaf alone
    // would allow an intermediate directory to be replaced with a symlink.
    let path = std::path::absolute(path)?;
    // macOS publishes /tmp, /var and /etc as aliases to root-owned /private directories.
    #[cfg(target_os = "macos")]
    let path = if ["/tmp", "/var", "/etc"].iter().any(|p| path.starts_with(p)) {
        Path::new("/private").join(path.strip_prefix("/").map_err(io::Error::other)?)
    } else {
        path
    };
    let base =
        OFlags::RDONLY | OFlags::NONBLOCK | OFlags::NOFOLLOW | OFlags::CLOEXEC | OFlags::NOCTTY;
    let mut parent = open("/", base | OFlags::DIRECTORY, Mode::empty())?;
    let mut components = path.components().peekable();
    while let Some(component) = components.next() {
        match component {
            Component::RootDir | Component::CurDir => continue,
            Component::Normal(_) | Component::ParentDir => {}
            Component::Prefix(_) => return Err(io::Error::other("invalid input path")),
        }
        let name = component.as_os_str();
        if components.peek().is_none() {
            return Ok(openat(parent, name, base, Mode::empty())?.into());
        }
        parent = openat(&parent, name, base | OFlags::DIRECTORY, Mode::empty())?;
    }
    Err(io::Error::other("expected file path"))
}

#[cfg(not(unix))]
fn open_regular(_path: &Path) -> io::Result<File> {
    // Neither negotiated binary runs on this platform. Avoid opening device paths or pipes.
    Err(io::ErrorKind::Unsupported.into())
}

pub(super) fn read(path: &Path, limit: u64) -> io::Result<Vec<u8>> {
    let file = open_regular(path)?;
    let meta = file.metadata()?;
    if !meta.is_file() || meta.len() > limit {
        return Err(io::Error::other("input exceeds regular-file bounds"));
    }
    let mut bytes = Vec::new();
    file.take(limit + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > limit {
        return Err(io::Error::other("input grew beyond bound"));
    }
    Ok(bytes)
}

pub(super) fn required(path: &Path, name: &'static str) -> Result<Vec<u8>, VerificationError> {
    read(path, PROFILE_LIMIT).map_err(|error| input_error(error, name))
}

fn input_error(error: io::Error, name: &'static str) -> VerificationError {
    if error.kind() == io::ErrorKind::Unsupported {
        VerificationError::UnsupportedPlatform
    } else {
        VerificationError::Input(name)
    }
}

pub(super) fn optional(
    path: Option<&Path>,
    name: &'static str,
) -> Result<Option<Vec<u8>>, VerificationError> {
    match path.map(|path| read(path, PROFILE_LIMIT)) {
        None => Ok(None),
        Some(Ok(bytes)) => Ok(Some(bytes)),
        Some(Err(err)) if err.kind() == io::ErrorKind::NotFound => Ok(None),
        Some(Err(err)) => Err(input_error(err, name)),
    }
}

#[cfg(all(test, unix))]
mod tests {
    use std::fs;
    use std::os::unix::fs::symlink;

    use std::process::Command;

    use super::*;

    #[test]
    fn bounds_regular_files_and_rejects_symlinks_directories_and_pipes() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().canonicalize().unwrap();
        let real_dir = root.join("real");
        fs::create_dir(&real_dir).unwrap();
        let file = real_dir.join("input");
        fs::write(&file, [0x00; 4]).unwrap();
        assert_eq!(read(&file, 4).unwrap(), [0x00; 4]);
        assert!(read(&file, 3).is_err());
        assert!(read(&real_dir, 4).is_err());
        assert_eq!(
            optional(Some(&root.join("missing")), "input").unwrap(),
            None
        );

        let link = root.join("link");
        symlink(&file, &link).unwrap();
        assert!(read(&link, 4).is_err());
        let parent_link = root.join("directory-link");
        symlink(&real_dir, &parent_link).unwrap();
        assert!(read(&parent_link.join("input"), 4).is_err());

        let fifo = root.join("fifo");
        assert!(
            Command::new("mkfifo")
                .arg(&fifo)
                .status()
                .unwrap()
                .success()
        );
        assert!(read(&fifo, 4).is_err());
    }
}
