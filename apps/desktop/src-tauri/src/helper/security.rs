//! Explicit protected current-user DACLs for both discovery and pipe instances.
use std::{ffi::c_void, path::Path, ptr};
use windows_sys::Win32::{
    Foundation::{CloseHandle, GetLastError, LocalFree},
    Security::{
        Authorization::{
            ConvertSidToStringSidW, ConvertStringSecurityDescriptorToSecurityDescriptorW,
            SDDL_REVISION_1,
        },
        DACL_SECURITY_INFORMATION, GetTokenInformation, PROTECTED_DACL_SECURITY_INFORMATION,
        SECURITY_ATTRIBUTES, SetFileSecurityW, TOKEN_QUERY, TOKEN_USER, TokenUser,
    },
    System::Threading::{GetCurrentProcess, GetProcessTimes, OpenProcessToken},
};

pub struct UserSecurity(pub *mut c_void);
impl UserSecurity {
    pub fn new() -> Result<Self, String> {
        unsafe {
            let mut token = ptr::null_mut();
            if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) == 0 {
                return Err(error());
            }
            let mut size = 0;
            GetTokenInformation(token, TokenUser, ptr::null_mut(), 0, &mut size);
            let mut buffer = vec![0usize; (size as usize).div_ceil(std::mem::size_of::<usize>())];
            let ok = GetTokenInformation(
                token,
                TokenUser,
                buffer.as_mut_ptr().cast(),
                size,
                &mut size,
            );
            CloseHandle(token);
            if ok == 0 {
                return Err(error());
            }
            let user = &*buffer.as_ptr().cast::<TOKEN_USER>();
            let mut sid = ptr::null_mut();
            if ConvertSidToStringSidW(user.User.Sid, &mut sid) == 0 {
                return Err(error());
            }
            let mut len = 0;
            while *sid.add(len) != 0 {
                len += 1;
            }
            let sid_text = String::from_utf16_lossy(std::slice::from_raw_parts(sid, len));
            LocalFree(sid.cast());
            let sddl: Vec<u16> = format!("D:P(A;OICI;GA;;;{sid_text})\0")
                .encode_utf16()
                .collect();
            let mut descriptor = ptr::null_mut();
            if ConvertStringSecurityDescriptorToSecurityDescriptorW(
                sddl.as_ptr(),
                SDDL_REVISION_1,
                &mut descriptor,
                ptr::null_mut(),
            ) == 0
            {
                return Err(error());
            }
            Ok(Self(descriptor))
        }
    }
    pub fn attributes(&self) -> SECURITY_ATTRIBUTES {
        SECURITY_ATTRIBUTES {
            nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
            lpSecurityDescriptor: self.0,
            bInheritHandle: 0,
        }
    }
    pub fn directory(&self, path: &Path) -> Result<(), String> {
        use std::os::windows::ffi::OsStrExt;
        let parent = path.parent().ok_or("directory has no parent")?;
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        let wide: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
        let attributes = self.attributes();
        if unsafe {
            windows_sys::Win32::Storage::FileSystem::CreateDirectoryW(wide.as_ptr(), &attributes)
        } == 0
            && !path.is_dir()
        {
            return Err(error());
        }
        self.apply(path)
    }
    pub fn apply(&self, path: &Path) -> Result<(), String> {
        use std::os::windows::ffi::OsStrExt;
        let path: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
        if unsafe {
            SetFileSecurityW(
                path.as_ptr(),
                DACL_SECURITY_INFORMATION | PROTECTED_DACL_SECURITY_INFORMATION,
                self.0,
            )
        } == 0
        {
            Err(error())
        } else {
            Ok(())
        }
    }
    #[cfg(test)]
    pub fn sddl(&self) -> String {
        unsafe {
            let mut text = ptr::null_mut();
            assert_ne!(windows_sys::Win32::Security::Authorization::ConvertSecurityDescriptorToStringSecurityDescriptorW(self.0, SDDL_REVISION_1, DACL_SECURITY_INFORMATION, &mut text, ptr::null_mut()), 0);
            let mut len = 0;
            while *text.add(len) != 0 {
                len += 1;
            }
            let result = String::from_utf16_lossy(std::slice::from_raw_parts(text, len));
            LocalFree(text.cast());
            result
        }
    }
}
impl Drop for UserSecurity {
    fn drop(&mut self) {
        unsafe {
            LocalFree(self.0);
        }
    }
}
#[cfg(test)]
pub fn handle_sddl(handle: windows_sys::Win32::Foundation::HANDLE) -> String {
    use windows_sys::Win32::Security::Authorization::{GetSecurityInfo, SE_KERNEL_OBJECT};
    unsafe {
        let mut descriptor = ptr::null_mut();
        assert_eq!(
            GetSecurityInfo(
                handle,
                SE_KERNEL_OBJECT,
                DACL_SECURITY_INFORMATION,
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
                &mut descriptor
            ),
            0
        );
        UserSecurity(descriptor).sddl()
    }
}
fn error() -> String {
    std::io::Error::from_raw_os_error(unsafe { GetLastError() } as i32).to_string()
}
pub fn process_started_at() -> Result<u64, String> {
    unsafe {
        let mut creation = std::mem::zeroed();
        let mut exit = std::mem::zeroed();
        let mut kernel = std::mem::zeroed();
        let mut user = std::mem::zeroed();
        if GetProcessTimes(
            GetCurrentProcess(),
            &mut creation,
            &mut exit,
            &mut kernel,
            &mut user,
        ) == 0
        {
            return Err(error());
        }
        Ok((u64::from(creation.dwHighDateTime) << 32) | u64::from(creation.dwLowDateTime))
    }
}
