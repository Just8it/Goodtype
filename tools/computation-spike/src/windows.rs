use std::ffi::{OsStr, c_void};
use std::fs;
use std::io;
use std::mem::{size_of, zeroed};
use std::net::{TcpListener, TcpStream};
use std::os::windows::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::ptr::{null, null_mut};
use std::time::Instant;

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
}

impl Profile {
    fn new() -> Result<Self, String> {
        let name = wide(format!("Goodtype.ComputationSpike.{}", std::process::id()));
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

    fn grant(&self, path: &Path, permission: &str) -> Result<(), String> {
        // Only freshly staged runtime/input/output directories receive this profile SID.
        let result = Command::new("icacls.exe")
            .arg(path)
            .arg("/grant")
            .arg(format!("*{}:(OI)(CI){permission}", self.sid_text))
            .arg("/T")
            .arg("/Q")
            .output()
            .map_err(|e| e.to_string())?;
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

impl Drop for Profile {
    fn drop(&mut self) {
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

fn execute(
    profile: &Profile,
    base: &Path,
    runtime: &Path,
    mode: &str,
    timeout_ms: u32,
) -> Result<serde_json::Value, String> {
    let output = base.join("output");
    let native_control = mode.starts_with("native-");
    let executable = wide(if native_control {
        base.join("input/native-smoke.exe")
    } else {
        runtime.join("python.exe")
    });
    let script = base.join("input/probe.py");
    let config = base.join("input/config.json");
    let mut command = wide(if native_control {
        "native-smoke.exe --native-smoke".to_owned()
    } else if mode == "python-bootstrap" {
        "python.exe -I -B -c \"open('bootstrap.txt', 'w').write('ok')\"".to_owned()
    } else {
        format!(
            "python.exe -I -B \"{}\" \"{}\" {mode}",
            script.display(),
            config.display()
        )
    });
    let cwd = wide(&output);
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
    startup.StartupInfo.wShowWindow = 0; // SW_HIDE: the experiment must not open a visible console.
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
    let started = Instant::now();
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
    let wait = unsafe { WaitForSingleObject(process.0, timeout_ms) };
    let timed_out = wait == WAIT_TIMEOUT;
    if timed_out {
        check(
            unsafe { TerminateJobObject(job.0, 124) },
            "TerminateJobObject",
        )?;
        check_wait(unsafe { WaitForSingleObject(process.0, 5000) })?;
    } else {
        check_wait(wait)?;
    }
    let mut exit_code = 0;
    check(
        unsafe { GetExitCodeProcess(process.0, &mut exit_code) },
        "GetExitCodeProcess",
    )?;
    let log_text = fs::read_to_string(output.join(format!("{mode}.log"))).unwrap_or_default();
    Ok(
        serde_json::json!({"mode":mode, "exit_code":exit_code, "timed_out":timed_out,
        "elapsed_ms":started.elapsed().as_millis(), "log":log_text}),
    )
}

fn check_wait(value: u32) -> Result<(), String> {
    if value == WAIT_OBJECT_0 {
        Ok(())
    } else {
        Err(format!("Unexpected process wait: {value:#x}"))
    }
}

pub fn run() -> Result<(), String> {
    let base = PathBuf::from(
        std::env::args_os()
            .nth(1)
            .ok_or("Usage: goodtype-computation-spike <prepared-directory>")?,
    )
    .canonicalize()
    .map_err(|e| e.to_string())?;
    if !base.join("runtime-manifest.json").is_file() || !base.join("runtime/python.exe").is_file() {
        return Err("Prepare the isolated experimental runtime first.".into());
    }
    let runtime = base.join("runtime");
    let base = base
        .join("trials")
        .join(format!("probe-{}", std::process::id()));
    if base.exists() {
        return Err("Trial directory already exists. Preserve previous evidence.".into());
    }
    fs::create_dir_all(&base).map_err(|e| e.to_string())?;
    for name in [
        "input",
        "output",
        "private",
        "other-notebook",
        "canonical",
        "package-cache",
        "public-app",
    ] {
        fs::create_dir(base.join(name)).map_err(|e| e.to_string())?;
    }
    fs::write(base.join("input/probe.py"), include_str!("../probe.py"))
        .map_err(|e| e.to_string())?;
    fs::copy(
        std::env::current_exe().map_err(|e| e.to_string())?,
        base.join("input/native-smoke.exe"),
    )
    .map_err(|e| e.to_string())?;
    fs::write(base.join("input/value.txt"), "approved input").map_err(|e| e.to_string())?;
    let mut canaries = Vec::new();
    for directory in [
        "private",
        "other-notebook",
        "canonical",
        "package-cache",
        "public-app",
    ] {
        let path = base.join(directory).join("canary.txt");
        fs::write(&path, "synthetic canary, not user data").map_err(|e| e.to_string())?;
        canaries.push(path);
    }
    let listener = TcpListener::bind(("127.0.0.1", 0)).map_err(|e| e.to_string())?;
    let address = listener.local_addr().map_err(|e| e.to_string())?;
    let baseline = TcpStream::connect(address).map_err(|e| e.to_string())?;
    drop(listener.accept().map_err(|e| e.to_string())?);
    drop(baseline);
    listener.set_nonblocking(true).map_err(|e| e.to_string())?;
    let registry_canary = format!("Software\\Goodtype\\ComputationProbe-{}", std::process::id());
    let registry_path = format!("HKCU\\{registry_canary}");
    let staged = Command::new("reg.exe").args(["add", &registry_path, "/v", "canary", "/d", "synthetic canary", "/f"]).output().map_err(|error| error.to_string())?;
    if !staged.status.success() { return Err("could not stage registry canary".into()); }
    let config = serde_json::json!({"registry_canary":registry_canary,"base":base, "runtime":runtime, "canaries":canaries, "parent_pid":std::process::id(), "canary_port":address.port()});
    fs::write(
        base.join("input/config.json"),
        serde_json::to_vec(&config).unwrap(),
    )
    .map_err(|e| e.to_string())?;
    let profile = Profile::new()?;
    let public_canary = Command::new("icacls.exe")
        .arg(base.join("public-app"))
        .args(["/grant", "*S-1-15-2-1:(OI)(CI)RX", "/T", "/Q"])
        .output()
        .map_err(|e| e.to_string())?;
    if !public_canary.status.success() {
        return Err("Could not stage all-application-packages canary".into());
    }
    profile.grant(&runtime, "RX")?;
    profile.grant(&base.join("input"), "RX")?;
    profile.grant(&base.join("output"), "M")?;
    // Give the output directory a low-integrity write label. No other directory receives it.
    let label = Command::new("icacls.exe")
        .arg(base.join("output"))
        .args(["/setintegritylevel", "(OI)(CI)L", "/T", "/Q"])
        .output()
        .map_err(|e| e.to_string())?;
    if !label.status.success() {
        return Err(String::from_utf8_lossy(&label.stderr).into_owned());
    }
    let mut results = Vec::new();
    for (mode, timeout) in [
        ("native-smoke", 30_000),
        ("python-bootstrap", 10_000),
        ("isolation", 30_000),
        ("science", 60_000),
        ("session", 30_000),
        ("memory", 10_000),
        ("loop", 1500),
    ] {
        let result = execute(&profile, &base, &runtime, mode, timeout)?;
        println!(
            "{mode}: exit={}, timeout={}, elapsed={} ms",
            result["exit_code"], result["timed_out"], result["elapsed_ms"]
        );
        results.push(result);
    }
    let cleanup = Command::new("reg.exe").args(["delete", &registry_path, "/f"]).output().map_err(|error| error.to_string())?;
    if !cleanup.status.success() { return Err("could not remove synthetic registry canary".into()); }
    let unchanged = canaries.iter().all(|path| {
        fs::read_to_string(path).ok().as_deref() == Some("synthetic canary, not user data")
    });
    let no_canary_connection = match listener.accept() {
        Err(error) if error.kind() == io::ErrorKind::WouldBlock => true,
        Ok(_) => false,
        Err(error) => return Err(format!("Canary listener: {error}")),
    };
    let passed = unchanged
        && no_canary_connection
        && results.iter().all(|result| {
            if result["mode"] == "loop" {
                result["timed_out"] == true
            } else {
                result["exit_code"] == 0 && result["timed_out"] == false
            }
        });
    let evidence = serde_json::json!({"experimental_only":true,"passed":passed,"network_canary_baseline_reachable":true,"sandbox_canary_connected":!no_canary_connection,
        "canaries_unchanged":unchanged,"results":results});
    fs::write(
        base.join("evidence.json"),
        serde_json::to_vec_pretty(&evidence).unwrap(),
    )
    .map_err(|e| e.to_string())?;
    println!("Evidence: {}", base.join("evidence.json").display());
    if passed {
        Ok(())
    } else {
        Err(format!(
            "Probe gate failed. Inspect {}",
            base.join("evidence.json").display()
        ))
    }
}
