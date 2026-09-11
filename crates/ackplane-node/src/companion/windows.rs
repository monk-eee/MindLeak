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
    use windows_sys::Win32::{
        Foundation::GENERIC_ALL,
        Security::{EqualSid, GetAce, ACCESS_ALLOWED_ACE, ACCESS_DENIED_ACE, SE_DACL_PROTECTED},
    };

    // Windows may serialize a SID using an alias; compare the actual owner and ACEs, not SDDL text.
    #[test]
    fn pipe_access_names_the_process_user_not_the_default_owner_group() {
        let sid = current_user_sid().unwrap();
        let descriptor = descriptor().unwrap();
        let expected =
            SecurityDescriptor::deserialize(&U16CString::from_str(format!("O:{sid}")).unwrap())
                .unwrap();
        let network =
            SecurityDescriptor::deserialize(&U16CString::from_str("O:NU").unwrap()).unwrap();
        let owner = descriptor.owner().unwrap().0;
        let expected_owner = expected.owner().unwrap().0;
        let network_identity = network.owner().unwrap().0;
        let (acl, defaulted) = descriptor
            .dacl()
            .unwrap()
            .expect("the pipe must have a DACL");
        assert!(!owner.is_null() && !expected_owner.is_null() && !network_identity.is_null());
        assert!(
            !acl.is_null() && !defaulted,
            "a null/default DACL could grant unintended access"
        );
        assert_ne!(
            descriptor.control_and_revision().unwrap().0 & SE_DACL_PROTECTED,
            0
        );
        // These pointers are owned by the live descriptors; GetAce returns an entry in that ACL.
        unsafe {
            assert_ne!(EqualSid(owner.cast_mut(), expected_owner.cast_mut()), 0);
            assert_eq!(
                (*acl).AceCount,
                2,
                "there must be no extra principal grants"
            );
            let mut entry = ptr::null_mut();
            assert_ne!(GetAce(acl.cast_mut(), 0, &mut entry), 0);
            let denied = &*entry.cast::<ACCESS_DENIED_ACE>();
            assert_eq!(denied.Header.AceType, 1);
            assert_eq!(denied.Header.AceFlags, 0);
            assert_eq!(denied.Mask, GENERIC_ALL);
            assert_ne!(
                EqualSid(
                    ptr::addr_of!(denied.SidStart).cast_mut().cast(),
                    network_identity.cast_mut()
                ),
                0
            );
            assert_ne!(GetAce(acl.cast_mut(), 1, &mut entry), 0);
            let allowed = &*entry.cast::<ACCESS_ALLOWED_ACE>();
            assert_eq!(allowed.Header.AceType, 0);
            assert_eq!(allowed.Header.AceFlags, 0);
            assert_eq!(allowed.Mask, GENERIC_ALL);
            assert_ne!(
                EqualSid(
                    ptr::addr_of!(allowed.SidStart).cast_mut().cast(),
                    expected_owner.cast_mut()
                ),
                0
            );
        }
    }
}
