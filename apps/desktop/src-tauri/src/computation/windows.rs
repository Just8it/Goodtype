use std::ffi::{OsStr, c_void};
use std::io;
use std::mem::{size_of, size_of_val, zeroed};
use std::os::windows::{ffi::OsStrExt, process::CommandExt};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::ptr::{null, null_mut};

use windows_sys::Win32::Foundation::*;
use windows_sys::Win32::Security::Authorization::ConvertSidToStringSidW;
use windows_sys::Win32::Security::Isolation::{
    CreateAppContainerProfile, DeleteAppContainerProfile,
};
use windows_sys::Win32::Security::{
    DeriveCapabilitySidsFromName, FreeSid, PSID, SECURITY_CAPABILITIES, SID_AND_ATTRIBUTES,
};
use windows_sys::Win32::System::JobObjects::*;
use windows_sys::Win32::System::Threading::*;

fn wide(value: impl AsRef<OsStr>) -> Vec<u16> {
    value.as_ref().encode_wide().chain(Some(0)).collect()
}

fn check(result: i32, operation: &str) -> Result<(), String> {
    if result == 0 {
        Err(format!("{operation}: {}", io::Error::last_os_error()))
    } else {
        Ok(())
    }
}

struct Handle(HANDLE);
// Windows kernel handles support concurrent waits, queries and termination.
unsafe impl Send for Handle {}
unsafe impl Sync for Handle {}
impl Drop for Handle {
    fn drop(&mut self) {
        unsafe { CloseHandle(self.0) };
    }
}

struct Profile {
    name: Vec<u16>,
    sid: PSID,
    sid_text: String,
    capabilities: Vec<SID_AND_ATTRIBUTES>,
    grants: Vec<PathBuf>,
}

impl Profile {
    fn new() -> Result<Self, String> {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|duration| duration.as_nanos())
            .unwrap_or(0);
        let name = wide(format!(
            "Goodtype.Calculation.{}.{}.{}",
            std::process::id(),
            timestamp,
            NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        let mut sid = null_mut();
        let result = unsafe {
            CreateAppContainerProfile(
                name.as_ptr(),
                name.as_ptr(),
                name.as_ptr(),
                null(),
                0,
                &mut sid,
            )
        };
        if result < 0 {
            return Err(format!("CreateAppContainerProfile HRESULT {result:#x}"));
        }
        let mut profile = Self {
            name,
            sid,
            sid_text: String::new(),
            capabilities: Vec::new(),
            grants: Vec::new(),
        };
        let mut text = null_mut();
        check(
            unsafe { ConvertSidToStringSidW(sid, &mut text) },
            "ConvertSidToStringSidW",
        )?;
        unsafe {
            let length = (0..).take_while(|index| *text.add(*index) != 0).count();
            profile.sid_text = String::from_utf16_lossy(std::slice::from_raw_parts(text, length));
            LocalFree(text.cast());
        }
        for name in ["registryRead"] {
            let mut groups = null_mut();
            let mut group_count = 0;
            let mut capabilities = null_mut();
            let mut count = 0;
            check(
                unsafe {
                    DeriveCapabilitySidsFromName(
                        wide(name).as_ptr(),
                        &mut groups,
                        &mut group_count,
                        &mut capabilities,
                        &mut count,
                    )
                },
                "DeriveCapabilitySidsFromName",
            )?;
            unsafe {
                for index in 0..group_count {
                    LocalFree((*groups.add(index as usize)).cast());
                }
                LocalFree(groups.cast());
                for index in 0..count {
                    profile.capabilities.push(SID_AND_ATTRIBUTES {
                        Sid: *capabilities.add(index as usize),
                        Attributes: 4,
                    });
                }
                LocalFree(capabilities.cast());
            }
        }
        Ok(profile)
    }

    fn grant(&mut self, path: &Path, permission: &str) -> Result<(), String> {
        // Only freshly staged runtime/input/output directories receive this profile SID.
        let result = Command::new("icacls.exe")
            .creation_flags(CREATE_NO_WINDOW)
            .arg(path)
            .arg("/grant")
            .arg(format!("*{}:(OI)(CI){permission}", self.sid_text))
            .arg("/T")
            .arg("/Q")
            .output()
            .map_err(|e| e.to_string())?;
        self.grants.push(path.to_path_buf());
        if !result.status.success() {
            return Err(format!(
                "icacls {}: {}",
                path.display(),
                String::from_utf8_lossy(&result.stderr)
            ));
        }
        Ok(())
    }
}

// The owned SID allocations remain immutable after process creation.
unsafe impl Send for Profile {}
unsafe impl Sync for Profile {}

impl Drop for Profile {
    fn drop(&mut self) {
        // Revoking walks every file of the runtime (thousands with NumPy, SciPy and Matplotlib) and
        // took ~15 s, during which ending a session or changing Python appeared to hang. The
        // process has already ended (`Process` stops it before its fields drop) and the profile is
        // deleted below, so the ACEs name a SID nothing can run as any more; removing them can
        // finish in the background.
        let grants = std::mem::take(&mut self.grants);
        let sid = std::mem::take(&mut self.sid_text);
        if !grants.is_empty() {
            std::thread::spawn(move || {
                for path in grants {
                    let _ = Command::new("icacls.exe")
                        .creation_flags(CREATE_NO_WINDOW)
                        .arg(&path)
                        .args(["/remove:g", &format!("*{sid}"), "/T", "/Q"])
                        .output();
                }
            });
        }
        unsafe {
            let result = DeleteAppContainerProfile(self.name.as_ptr());
            if result < 0 {
                eprintln!("Profile cleanup failed: {result:#x}");
            }
            FreeSid(self.sid);
            for capability in &self.capabilities {
                LocalFree(capability.Sid.cast());
            }
        }
    }
}

struct Attributes {
    // usize storage keeps the opaque attribute list properly aligned.
    _storage: Vec<usize>,
    pointer: LPPROC_THREAD_ATTRIBUTE_LIST,
}

impl Attributes {
    fn new() -> Result<Self, String> {
        let mut bytes = 0;
        unsafe { InitializeProcThreadAttributeList(null_mut(), 2, 0, &mut bytes) };
        let mut storage = vec![0; bytes.div_ceil(size_of::<usize>())];
        let pointer = storage.as_mut_ptr().cast();
        check(
            unsafe { InitializeProcThreadAttributeList(pointer, 2, 0, &mut bytes) },
            "InitializeProcThreadAttributeList",
        )?;
        Ok(Self {
            _storage: storage,
            pointer,
        })
    }

    fn set<T>(&mut self, key: u32, value: &T) -> Result<(), String> {
        check(
            unsafe {
                UpdateProcThreadAttribute(
                    self.pointer,
                    0,
                    key as usize,
                    (value as *const T).cast::<c_void>(),
                    size_of::<T>(),
                    null_mut(),
                    null(),
                )
            },
            "UpdateProcThreadAttribute",
        )
    }
}

impl Drop for Attributes {
    fn drop(&mut self) {
        unsafe { DeleteProcThreadAttributeList(self.pointer) };
    }
}

/// A restricted Python process. Closing it terminates its entire one-process job first.
pub struct Process {
    process: Handle,
    job: Handle,
    _profile: Profile,
}

impl Process {
    pub fn start(runtime: &Path, input: &Path, scratch: &Path) -> Result<Self, String> {
        let mut profile = Profile::new()?;
        profile.grant(runtime, "RX")?;
        profile.grant(input, "RX")?;
        profile.grant(scratch, "M")?;
        let label = Command::new("icacls.exe")
            .creation_flags(CREATE_NO_WINDOW)
            .arg(scratch)
            .args(["/setintegritylevel", "(OI)(CI)L", "/T", "/Q"])
            .output()
            .map_err(|error| error.to_string())?;
        if !label.status.success() {
            return Err("could not restrict the Python scratch directory".into());
        }
        let executable = wide(runtime.join("python.exe"));
        let mut command = wide(format!(
            "python.exe -I -S -B -X utf8 \"{}\" \"{}\"",
            input.join("runner.py").display(),
            input.display()
        ));
        let output = scratch;
        let cwd = wide(output);
        let mut environment = [
            // Windows resolves profile storage from this value while creating the AppContainer.
            format!(
                "LOCALAPPDATA={}",
                std::env::var("LOCALAPPDATA").map_err(|e| e.to_string())?
            ),
            format!("MPLCONFIGDIR={}", output.display()),
            "OPENBLAS_NUM_THREADS=1".to_owned(),
            "PATH=".to_owned(),
            "PYTHONHASHSEED=0".to_owned(),
            "PYTHONUTF8=1".to_owned(),
            "SYSTEMROOT=C:\\Windows".to_owned(),
            format!("TEMP={}", output.display()),
            format!("TMP={}", output.display()),
            format!(
                "USERPROFILE={}",
                std::env::var("USERPROFILE").map_err(|e| e.to_string())?
            ),
        ]
        .to_vec();
        for key in [
            "ALLUSERSPROFILE",
            "APPDATA",
            "COMSPEC",
            "PROGRAMDATA",
            "SYSTEMDRIVE",
            "WINDIR",
        ] {
            if let Ok(value) = std::env::var(key) {
                environment.push(format!("{key}={value}"));
            }
        }
        environment.push(format!("IPYTHONDIR={}", output.display()));
        environment.sort_by_key(|entry| entry.to_uppercase());
        let mut environment = wide(environment.join("\0"));
        environment.push(0);
        let capabilities = SECURITY_CAPABILITIES {
            AppContainerSid: profile.sid,
            Capabilities: profile.capabilities.as_ptr().cast_mut(),
            CapabilityCount: profile.capabilities.len() as u32,
            Reserved: 0,
        };
        // The one-process Job Object denies descendants without the incompatible creation mitigation.
        let mut attributes = Attributes::new()?;
        attributes.set(PROC_THREAD_ATTRIBUTE_SECURITY_CAPABILITIES, &capabilities)?;
        let restricted_packages = 1u32;
        attributes.set(
            PROC_THREAD_ATTRIBUTE_ALL_APPLICATION_PACKAGES_POLICY,
            &restricted_packages,
        )?;
        let mut startup: STARTUPINFOEXW = unsafe { zeroed() };
        startup.StartupInfo.cb = size_of::<STARTUPINFOEXW>() as u32;
        startup.StartupInfo.dwFlags = STARTF_USESHOWWINDOW;
        startup.StartupInfo.wShowWindow = 0; // SW_HIDE: the runner must not open a visible console.
        startup.lpAttributeList = attributes.pointer;
        let job = Handle(unsafe { CreateJobObjectW(null(), null()) });
        if job.0.is_null() {
            return Err(io::Error::last_os_error().to_string());
        }
        let mut limits: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = unsafe { zeroed() };
        limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE
            | JOB_OBJECT_LIMIT_PROCESS_MEMORY
            | JOB_OBJECT_LIMIT_ACTIVE_PROCESS;
        limits.BasicLimitInformation.ActiveProcessLimit = 1;
        limits.ProcessMemoryLimit = 1024 * 1024 * 1024;
        check(
            unsafe {
                SetInformationJobObject(
                    job.0,
                    JobObjectExtendedLimitInformation,
                    (&limits as *const JOBOBJECT_EXTENDED_LIMIT_INFORMATION).cast(),
                    size_of_val(&limits) as u32,
                )
            },
            "SetInformationJobObject",
        )?;
        let mut info: PROCESS_INFORMATION = unsafe { zeroed() };
        check(
            unsafe {
                CreateProcessW(
                    executable.as_ptr(),
                    command.as_mut_ptr(),
                    null(),
                    null(),
                    0,
                    CREATE_SUSPENDED
                        | CREATE_NO_WINDOW
                        | CREATE_UNICODE_ENVIRONMENT
                        | EXTENDED_STARTUPINFO_PRESENT,
                    environment.as_ptr().cast(),
                    cwd.as_ptr(),
                    &startup.StartupInfo,
                    &mut info,
                )
            },
            "CreateProcessW",
        )?;
        let process = Handle(info.hProcess);
        let thread = Handle(info.hThread);
        if let Err(error) = check(
            unsafe { AssignProcessToJobObject(job.0, process.0) },
            "AssignProcessToJobObject",
        ) {
            unsafe { TerminateProcess(process.0, 1) };
            return Err(error);
        }
        if unsafe { ResumeThread(thread.0) } == u32::MAX {
            return Err(format!("ResumeThread: {}", io::Error::last_os_error()));
        }

        Ok(Self {
            process,
            job,
            _profile: profile,
        })
    }

    pub fn stop(&self) {
        unsafe {
            TerminateJobObject(self.job.0, 124);
            WaitForSingleObject(self.process.0, 5000);
        }
    }

    pub fn running(&self) -> bool {
        unsafe { WaitForSingleObject(self.process.0, 0) == WAIT_TIMEOUT }
    }
}

impl Drop for Process {
    fn drop(&mut self) {
        self.stop();
    }
}
