use std::{
    io,
    mem::size_of,
    os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle},
    ptr,
};

use interprocess::os::windows::security_descriptor::SecurityDescriptor;
use widestring::{U16CStr, U16CString};
use windows_sys::Win32::{
    Foundation::{GetLastError, LocalFree, ERROR_INSUFFICIENT_BUFFER},
    Security::{
        Authorization::ConvertSidToStringSidW, GetTokenInformation, TokenUser, TOKEN_QUERY,
        TOKEN_USER,
    },
    System::Threading::{GetCurrentProcess, OpenProcessToken},
};

pub(super) fn descriptor() -> io::Result<SecurityDescriptor> {
    let sid = current_user_sid()?;
    let sddl = U16CString::from_str(format!("O:{sid}D:P(D;;GA;;;NU)(A;;GA;;;{sid})"))
        .map_err(io::Error::other)?;
    SecurityDescriptor::deserialize(&sddl)
}

fn current_user_sid() -> io::Result<String> {
    let mut handle = ptr::null_mut();
    // The API initializes an owned token handle only on success.
    if unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut handle) } == 0 {
        return Err(io::Error::last_os_error());
    }
    let token = unsafe { OwnedHandle::from_raw_handle(handle) };
    let mut length = 0;
    unsafe {
        GetTokenInformation(
            token.as_raw_handle(),
            TokenUser,
            ptr::null_mut(),
            0,
            &mut length,
        )
    };
    if unsafe { GetLastError() } != ERROR_INSUFFICIENT_BUFFER {
        return Err(io::Error::last_os_error());
    }
    if (length as usize) < size_of::<TOKEN_USER>() || length > 65_536 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "invalid process identity size",
        ));
    }
    let mut storage = vec![0usize; (length as usize).div_ceil(size_of::<usize>())];
    if unsafe {
        GetTokenInformation(
            token.as_raw_handle(),
            TokenUser,
            storage.as_mut_ptr().cast(),
            length,
            &mut length,
        )
    } == 0
    {
        return Err(io::Error::last_os_error());
    }
    // Word-aligned storage contains TOKEN_USER followed by its OS-validated SID.
    let user = unsafe { &*storage.as_ptr().cast::<TOKEN_USER>() };
    let mut text = ptr::null_mut();
    if unsafe { ConvertSidToStringSidW(user.User.Sid, &mut text) } == 0 {
        return Err(io::Error::last_os_error());
    }
    let sid = unsafe { U16CStr::from_ptr_str(text) }.to_string_lossy();
    unsafe { LocalFree(text.cast()) };
    Ok(sid)
}

#[cfg(test)]
mod tests {
    use super::*;
    use interprocess::os::windows::security_descriptor::AsSecurityDescriptorExt;
    use windows_sys::Win32::Security::{DACL_SECURITY_INFORMATION, OWNER_SECURITY_INFORMATION};

    #[test]
    fn pipe_access_names_the_process_user_not_the_default_owner_group() {
        let sid = current_user_sid().unwrap();
        let descriptor = descriptor().unwrap();
        let sddl = descriptor
            .serialize(
                DACL_SECURITY_INFORMATION | OWNER_SECURITY_INFORMATION,
                U16CStr::to_string_lossy,
            )
            .unwrap();
        assert!(sddl.contains(&format!("O:{sid}")));
        assert!(sddl.contains(&format!("(A;;GA;;;{sid})")));
        assert!(sddl.contains("(D;;GA;;;NU)"));
        assert!(!sddl.contains(";;;OW)") && !sddl.contains(";;;BA)") && !sddl.contains(";;;WD)"));
    }
}
