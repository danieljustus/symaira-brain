#![allow(unsafe_code)]

use cap_fs_ext::{FollowSymlinks, OpenOptionsFollowExt};
use cap_std::fs::{Dir, File, OpenOptions};
use std::io;
use std::os::windows::io::AsRawHandle;
use std::path::Path;
use std::ptr::null_mut;
use windows_sys::Win32::Foundation::LocalFree;
use windows_sys::Win32::Security::Authorization::{
    GetSecurityInfo, SE_FILE_OBJECT, SetSecurityInfo,
};
use windows_sys::Win32::Security::{
    DACL_SECURITY_INFORMATION, GROUP_SECURITY_INFORMATION, GetSecurityDescriptorControl,
    OWNER_SECURITY_INFORMATION, PROTECTED_DACL_SECURITY_INFORMATION, SE_DACL_PROTECTED,
};

/// Copy the non-SACL security descriptor through retained file handles.
///
/// SACLs are intentionally excluded. Windows requires `SeSecurityPrivilege`
/// and `ACCESS_SYSTEM_SECURITY` for SACL access; ordinary adapter writes must
/// work without ambient privilege. A future privileged mode must opt into a
/// separate API and request SACL preservation explicitly.
pub(super) fn copy(parent: &Dir, name: &Path, destination: &File) -> io::Result<()> {
    let mut options = OpenOptions::new();
    options.read(true).follow(FollowSymlinks::No);
    let source = parent.open_with(name, &options)?;

    let mut owner = null_mut();
    let mut group = null_mut();
    let mut dacl = null_mut();
    let mut descriptor = null_mut();
    let security_information =
        OWNER_SECURITY_INFORMATION | GROUP_SECURITY_INFORMATION | DACL_SECURITY_INFORMATION;
    let status = unsafe {
        GetSecurityInfo(
            source.as_raw_handle(),
            SE_FILE_OBJECT,
            security_information,
            &raw mut owner,
            &raw mut group,
            &raw mut dacl,
            null_mut(),
            &raw mut descriptor,
        )
    };
    if status != 0 {
        return Err(io::Error::from_raw_os_error(status.cast_signed()));
    }
    let result = (|| {
        let mut control = 0_u16;
        let mut revision = 0_u32;
        let success = unsafe {
            GetSecurityDescriptorControl(descriptor, &raw mut control, &raw mut revision)
        };
        if success == 0 {
            return Err(io::Error::last_os_error());
        }
        let mut information = security_information;
        if control & SE_DACL_PROTECTED != 0 {
            information |= PROTECTED_DACL_SECURITY_INFORMATION;
        }
        let status = unsafe {
            SetSecurityInfo(
                destination.as_raw_handle(),
                SE_FILE_OBJECT,
                information,
                owner,
                group,
                dacl,
                null_mut(),
            )
        };
        if status != 0 {
            return Err(io::Error::from_raw_os_error(status.cast_signed()));
        }
        let _ = revision;
        Ok(())
    })();
    let _ = unsafe { LocalFree(descriptor.cast()) };
    result
}
