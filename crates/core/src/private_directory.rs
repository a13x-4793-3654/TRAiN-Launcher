//! Owner-only temporary staging shared by executable installers and private tickets.

#[cfg(any(unix, test))]
use std::fs;
use std::fs::{File, OpenOptions};
use std::io;
use std::path::{Component, Path};

pub struct PrivateDirectory {
    directory: Option<tempfile::TempDir>,
    #[cfg(windows)]
    lock: Option<File>,
}

impl PrivateDirectory {
    pub fn new(prefix: &str) -> io::Result<Self> {
        Self::create_in(prefix, &std::env::temp_dir())
    }

    fn create_in(prefix: &str, parent: &Path) -> io::Result<Self> {
        #[cfg(unix)]
        let parent = parent.canonicalize()?;
        let directory = tempfile::Builder::new()
            .prefix(prefix)
            .rand_bytes(16)
            .tempdir_in(&parent)?;
        #[cfg(unix)]
        validate_parent(&parent, fs::metadata(directory.path())?.uid())?;

        #[cfg(windows)]
        let lock = {
            use std::os::windows::fs::{MetadataExt, OpenOptionsExt};
            use windows_sys::Win32::Storage::FileSystem::{
                FILE_ATTRIBUTE_REPARSE_POINT, FILE_FLAG_BACKUP_SEMANTICS,
                FILE_FLAG_OPEN_REPARSE_POINT, FILE_SHARE_READ, FILE_SHARE_WRITE,
            };
            // Deny delete sharing while Java can still reopen files by path.
            let lock = OpenOptions::new()
                .read(true)
                .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE)
                .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT)
                .open(directory.path())?;
            let metadata = lock.metadata()?;
            if !metadata.is_dir() || metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
            {
                return Err(io::Error::other("temporary directory is a reparse point"));
            }
            lock
        };
        restrict_to_owner(directory.path())?;
        Ok(Self {
            directory: Some(directory),
            #[cfg(windows)]
            lock: Some(lock),
        })
    }

    pub fn path(&self) -> &Path {
        self.directory
            .as_ref()
            .expect("live private directory")
            .path()
    }

    pub fn create_file(&self, name: &str) -> io::Result<File> {
        let mut components = Path::new(name).components();
        if !matches!(components.next(), Some(Component::Normal(_))) || components.next().is_some() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "invalid staging file name",
            ));
        }
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        options.open(self.path().join(name))
    }
}

impl Drop for PrivateDirectory {
    fn drop(&mut self) {
        #[cfg(windows)]
        self.lock.take();
        if let Some(directory) = self.directory.take() {
            if let Err(err) = directory.close() {
                eprintln!("failed to remove private temporary directory: {err}");
            }
        }
    }
}

#[cfg(unix)]
use std::os::unix::fs::MetadataExt;

#[cfg(unix)]
fn validate_parent(parent: &Path, owner: u32) -> io::Result<()> {
    // 0700 alone is insufficient if another user can replace the directory in its parent.
    let parent = parent.canonicalize()?;
    for ancestor in parent.ancestors() {
        let metadata = fs::metadata(ancestor)?;
        if (metadata.uid() != owner && metadata.uid() != 0)
            || (metadata.mode() & 0o022 != 0 && metadata.mode() & 0o1000 == 0)
        {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "temporary directory has an unsafe owner or replaceable parent",
            ));
        }
    }
    Ok(())
}

#[cfg(unix)]
fn restrict_to_owner(path: &Path) -> io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o700))
}

#[cfg(windows)]
fn restrict_to_owner(path: &Path) -> io::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::{
        Foundation::{LocalFree, ERROR_INVALID_SECURITY_DESCR},
        Security::{
            Authorization::{
                ConvertStringSecurityDescriptorToSecurityDescriptorW, SetNamedSecurityInfoW,
                SE_FILE_OBJECT,
            },
            GetSecurityDescriptorDacl, DACL_SECURITY_INFORMATION,
            PROTECTED_DACL_SECURITY_INFORMATION,
        },
    };
    let sddl: Vec<u16> = "D:P(A;OICI;FA;;;OW)\0".encode_utf16().collect();
    let path: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
    unsafe {
        let mut descriptor = std::ptr::null_mut();
        if ConvertStringSecurityDescriptorToSecurityDescriptorW(
            sddl.as_ptr(),
            1,
            &mut descriptor,
            std::ptr::null_mut(),
        ) == 0
        {
            return Err(io::Error::last_os_error());
        }
        let mut dacl = std::ptr::null_mut();
        let mut present = 0;
        let mut defaulted = 0;
        let extracted =
            GetSecurityDescriptorDacl(descriptor, &mut present, &mut dacl, &mut defaulted);
        let result = if extracted != 0 && present != 0 && !dacl.is_null() {
            SetNamedSecurityInfoW(
                path.as_ptr(),
                SE_FILE_OBJECT,
                DACL_SECURITY_INFORMATION | PROTECTED_DACL_SECURITY_INFORMATION,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                dacl,
                std::ptr::null_mut(),
            )
        } else {
            ERROR_INVALID_SECURITY_DESCR
        };
        LocalFree(descriptor);
        if result != 0 {
            return Err(io::Error::from_raw_os_error(result as i32));
        }
    }
    Ok(())
}

#[cfg(not(any(unix, windows)))]
fn restrict_to_owner(_path: &Path) -> io::Result<()> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "owner-only staging is unavailable",
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn fresh_directories_ignore_precreated_shared_staging_and_cleanup() {
        let root = tempfile::tempdir().unwrap();
        let old = root.path().join("train-launcher-installers");
        fs::create_dir(&old).unwrap();
        let old_jar = old.join("forge-1.21-1-installer.jar");
        fs::write(&old_jar, b"untrusted").unwrap();
        let first = PrivateDirectory::create_in("train-installer-", root.path()).unwrap();
        let second = PrivateDirectory::create_in("train-installer-", root.path()).unwrap();
        assert_ne!(first.path(), second.path());
        assert_ne!(first.path(), old);
        first
            .create_file("installer.jar")
            .unwrap()
            .write_all(b"verified")
            .unwrap();
        assert_eq!(fs::read(&old_jar).unwrap(), b"untrusted");
        let path = first.path().to_path_buf();
        drop(first);
        assert!(!path.exists());
        assert!(second.path().exists());
        assert_eq!(fs::read(old_jar).unwrap(), b"untrusted");
    }

    #[test]
    fn installer_creation_is_exclusive_and_confined() {
        let directory = PrivateDirectory::new("train-installer-test-").unwrap();
        let mut file = directory.create_file("installer.jar").unwrap();
        file.write_all(b"first").unwrap();
        assert_eq!(
            directory.create_file("installer.jar").unwrap_err().kind(),
            io::ErrorKind::AlreadyExists
        );
        assert!(directory.create_file("../outside.jar").is_err());
        assert_eq!(
            fs::read(directory.path().join("installer.jar")).unwrap(),
            b"first"
        );
    }

    #[cfg(unix)]
    #[test]
    fn unix_permissions_and_replaceable_parent_and_link_attacks() {
        use std::os::unix::fs::{symlink, PermissionsExt};
        let root = tempfile::tempdir().unwrap();
        let directory = PrivateDirectory::create_in("train-installer-", root.path()).unwrap();
        let outside = root.path().join("outside.jar");
        fs::write(&outside, b"unchanged").unwrap();
        symlink(&outside, directory.path().join("installer.jar")).unwrap();
        assert_eq!(
            directory.create_file("installer.jar").unwrap_err().kind(),
            io::ErrorKind::AlreadyExists
        );
        assert_eq!(fs::read(&outside).unwrap(), b"unchanged");
        directory.create_file("private.jar").unwrap();
        assert_eq!(
            fs::metadata(directory.path()).unwrap().mode() & 0o777,
            0o700
        );
        assert_eq!(
            fs::metadata(directory.path().join("private.jar"))
                .unwrap()
                .mode()
                & 0o777,
            0o600
        );
        fs::set_permissions(root.path(), fs::Permissions::from_mode(0o777)).unwrap();
        assert!(PrivateDirectory::create_in("train-installer-", root.path()).is_err());
        fs::set_permissions(root.path(), fs::Permissions::from_mode(0o700)).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn replacing_a_temp_parent_symlink_cannot_redirect_staged_files() {
        use std::os::unix::fs::symlink;
        let root = tempfile::tempdir().unwrap();
        let alias = root.path().join("alias");
        let attacker = root.path().join("attacker");
        fs::create_dir(&attacker).unwrap();
        symlink(root.path(), &alias).unwrap();
        let directory = PrivateDirectory::create_in("train-installer-", &alias).unwrap();
        fs::remove_file(&alias).unwrap();
        symlink(&attacker, &alias).unwrap();
        directory.create_file("installer.jar").unwrap();
        assert_eq!(
            directory.path().parent().unwrap(),
            root.path().canonicalize().unwrap()
        );
        assert!(fs::read_dir(&attacker).unwrap().next().is_none());
    }

    #[cfg(windows)]
    #[test]
    fn windows_directory_cannot_be_replaced_until_guard_is_dropped() {
        let directory = PrivateDirectory::new("train-installer-test-").unwrap();
        let path = directory.path().to_path_buf();
        assert!(fs::rename(&path, path.with_extension("replaced")).is_err());
        assert!(fs::remove_dir(&path).is_err());
        directory.create_file("installer.jar").unwrap();
        assert_owner_only_acl(&path);
        assert_owner_only_acl(&path.join("installer.jar"));
        drop(directory);
        assert!(!path.exists());
    }

    #[cfg(windows)]
    fn assert_owner_only_acl(path: &Path) {
        use std::os::windows::ffi::OsStrExt;
        use windows_sys::Win32::{
            Foundation::LocalFree,
            Security::{
                Authorization::{
                    ConvertSecurityDescriptorToStringSecurityDescriptorW, GetNamedSecurityInfoW,
                    SE_FILE_OBJECT,
                },
                DACL_SECURITY_INFORMATION,
            },
        };
        unsafe {
            let path: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
            let mut descriptor = std::ptr::null_mut();
            assert_eq!(
                GetNamedSecurityInfoW(
                    path.as_ptr(),
                    SE_FILE_OBJECT,
                    DACL_SECURITY_INFORMATION,
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    &mut descriptor,
                ),
                0
            );
            let mut text = std::ptr::null_mut();
            assert_ne!(
                ConvertSecurityDescriptorToStringSecurityDescriptorW(
                    descriptor,
                    1,
                    DACL_SECURITY_INFORMATION,
                    &mut text,
                    std::ptr::null_mut(),
                ),
                0
            );
            let mut length = 0;
            while *text.add(length) != 0 {
                length += 1;
            }
            let sddl = String::from_utf16(std::slice::from_raw_parts(text, length)).unwrap();
            LocalFree(text.cast());
            LocalFree(descriptor);
            assert_eq!(sddl.matches('(').count(), 1, "{sddl}");
            assert!(sddl.contains(";;;OW)"), "{sddl}");
        }
    }
}
