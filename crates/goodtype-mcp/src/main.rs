#![cfg_attr(not(windows), allow(dead_code))]
use std::io::{self, Read, Write};

const NOT_RUNNING: &str = "Goodtype is not running or AI helpers are off. Open Goodtype, enable Settings â†’ AI helpers, then reconnect.";
const LINE_LIMIT: usize = 16 * 1024 * 1024;

struct Record {
    instance: String,
    pipe: String,
    token: String,
    pid: u32,
    started: u64,
}
fn parse_record(bytes: &[u8]) -> Result<Record, String> {
    let v: serde_json::Value = serde_json::from_slice(bytes).map_err(|_| NOT_RUNNING.to_owned())?;
    let string = |key| {
        v.get(key)
            .and_then(|v| v.as_str())
            .map(str::to_owned)
            .ok_or_else(|| NOT_RUNNING.to_owned())
    };
    let instance = string("instanceId")?;
    let token = string("token")?;
    let pipe = string("pipe")?;
    let hex = |s: &str, n| {
        s.len() == n
            && s.bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    };
    let pid = v["pid"]
        .as_u64()
        .and_then(|n| u32::try_from(n).ok())
        .filter(|n| *n != 0)
        .ok_or(NOT_RUNNING)?;
    let started = v["startedAt"]
        .as_u64()
        .filter(|n| *n != 0)
        .ok_or(NOT_RUNNING)?;
    if v["protocol"].as_u64() != Some(1)
        || !hex(&instance, 32)
        || !hex(&token, 64)
        || pipe != format!(r"\\.\pipe\goodtype-helper-{instance}")
    {
        return Err(NOT_RUNNING.into());
    }
    Ok(Record {
        instance,
        pipe,
        token,
        pid,
        started,
    })
}
fn validate_process(
    record: &Record,
    creation: impl FnOnce(u32) -> Option<u64>,
) -> Result<(), String> {
    if creation(record.pid) == Some(record.started) {
        Ok(())
    } else {
        Err(NOT_RUNNING.into())
    }
}
#[cfg(test)]
fn handshake(stream: &mut (impl Read + Write), record: &Record) -> Result<(), String> {
    handshake_launch(stream, record, None)
}
fn launch_token(args: &[String]) -> Result<Option<String>, String> {
    match args {
        [] => Ok(None),
        [flag, value]
            if flag == "--launch-token"
                && value.len() == 64
                && value.bytes().all(|b| b.is_ascii_hexdigit()) =>
        {
            Ok(Some(value.to_ascii_lowercase()))
        }
        _ => Err("Expected --launch-token <64 hex>".into()),
    }
}
fn handshake_launch(
    stream: &mut (impl Read + Write),
    record: &Record,
    launch: Option<&str>,
) -> Result<(), String> {
    let mut value = serde_json::json!({"type":"goodtype-relay-hello", "protocol":1,"instanceId":record.instance,"token":record.token,"relayVersion":env!("CARGO_PKG_VERSION")});
    if let Some(token) = launch {
        value["launchToken"] = token.into();
    }
    let mut hello = serde_json::to_vec(&value).map_err(|e| e.to_string())?;
    hello.push(b'\n');
    if hello.len() > 4096 {
        return Err("Relay hello exceeds 4096 bytes".into());
    }
    stream
        .write_all(&hello)
        .and_then(|()| stream.flush())
        .map_err(|e| e.to_string())?;
    // Read exactly through the newline: never consume the first MCP bytes.
    let mut line = Vec::new();
    loop {
        let mut byte = [0];
        stream
            .read_exact(&mut byte)
            .map_err(|_| "Goodtype closed during handshake")?;
        line.push(byte[0]);
        if line.len() > 4096 {
            return Err("Goodtype handshake exceeds 4096 bytes".into());
        }
        if byte[0] == b'\n' {
            break;
        }
    }
    let reply: serde_json::Value =
        serde_json::from_slice(&line).map_err(|_| "Malformed Goodtype handshake")?;
    match reply["type"].as_str() {
        Some("goodtype-relay-welcome") if reply["protocol"].as_u64() == Some(1) => Ok(()),
        Some("goodtype-relay-refused")
            if matches!(
                reply["reason"].as_str(),
                Some("bad_token" | "protocol_mismatch" | "disabled" | "busy")
            ) =>
        {
            Err(reply["message"]
                .as_str()
                .ok_or("Malformed Goodtype refusal")?
                .to_owned())
        }
        _ => Err("Malformed Goodtype handshake".into()),
    }
}
fn pump(mut input: impl Read, mut output: impl Write, limit: usize) -> io::Result<()> {
    let mut buffer = [0; 8192];
    let mut pending = 0usize;
    loop {
        let n = match input.read(&mut buffer) {
            Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
            result => result?,
        };
        if n == 0 {
            return Ok(());
        }
        for &byte in &buffer[..n] {
            if byte == b'\n' {
                pending = 0;
            } else {
                pending += 1;
            }
            if pending > limit {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "Relay line exceeds 16 MiB",
                ));
            }
        }
        output.write_all(&buffer[..n])?;
        output.flush()?;
    }
}
#[cfg(windows)]
mod windows {
    use super::*;
    use std::{
        fs::File,
        os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle},
        sync::Arc,
    };
    use windows_sys::Win32::{
        Foundation::{
            CloseHandle, ERROR_BROKEN_PIPE, ERROR_IO_PENDING, ERROR_PIPE_NOT_CONNECTED, FILETIME,
            GENERIC_READ, GENERIC_WRITE, INVALID_HANDLE_VALUE,
        },
        Storage::FileSystem::{
            CreateFileW, FILE_FLAG_OVERLAPPED, OPEN_EXISTING, ReadFile, WriteFile,
        },
        System::{
            IO::{GetOverlappedResult, OVERLAPPED},
            Pipes::GetNamedPipeServerProcessId,
            Threading::{
                CreateEventW, GetProcessTimes, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
            },
        },
    };
    #[cfg(test)]
    pub static READ_PENDING: std::sync::atomic::AtomicBool =
        std::sync::atomic::AtomicBool::new(false);

    pub struct PipeReader(Arc<OwnedHandle>);
    pub struct PipeWriter(Arc<OwnedHandle>);
    pub struct Pipe {
        reader: PipeReader,
        writer: PipeWriter,
    }
    impl Pipe {
        pub fn split(self) -> (PipeReader, PipeWriter) {
            (self.reader, self.writer)
        }
    }
    // SAFETY: callers keep the buffer valid until this function completes. Each call owns
    // its event and OVERLAPPED, and waits before either the buffer or OVERLAPPED can drop.
    unsafe fn transfer(
        handle: &OwnedHandle,
        buffer: *mut u8,
        len: usize,
        write: bool,
    ) -> io::Result<usize> {
        unsafe {
            let event = CreateEventW(std::ptr::null(), 1, 0, std::ptr::null());
            if event.is_null() {
                return Err(io::Error::last_os_error());
            }
            let event = OwnedHandle::from_raw_handle(event);
            let mut overlapped: OVERLAPPED = std::mem::zeroed();
            overlapped.hEvent = event.as_raw_handle();
            let len = len.min(u32::MAX as usize) as u32;
            let issued = if write {
                WriteFile(
                    handle.as_raw_handle(),
                    buffer,
                    len,
                    std::ptr::null_mut(),
                    &mut overlapped,
                )
            } else {
                ReadFile(
                    handle.as_raw_handle(),
                    buffer,
                    len,
                    std::ptr::null_mut(),
                    &mut overlapped,
                )
            };
            let mut error = if issued == 0 {
                Some(io::Error::last_os_error())
            } else {
                None
            };
            #[cfg(test)]
            if !write
                && error
                    .as_ref()
                    .is_some_and(|e| e.raw_os_error() == Some(ERROR_IO_PENDING as i32))
            {
                READ_PENDING.store(true, std::sync::atomic::Ordering::SeqCst);
            }
            let mut transferred = 0;
            if error
                .as_ref()
                .is_none_or(|e| e.raw_os_error() == Some(ERROR_IO_PENDING as i32))
            {
                error = if GetOverlappedResult(
                    handle.as_raw_handle(),
                    &overlapped,
                    &mut transferred,
                    1,
                ) == 0
                {
                    Some(io::Error::last_os_error())
                } else {
                    None
                };
            }
            match error {
                Some(e) if matches!(e.raw_os_error(), Some(code) if code == ERROR_BROKEN_PIPE as i32 || code == ERROR_PIPE_NOT_CONNECTED as i32) => {
                    if write {
                        Err(io::Error::new(io::ErrorKind::BrokenPipe, e))
                    } else {
                        Ok(0)
                    }
                }
                Some(e) => Err(e),
                None => Ok(transferred as usize),
            }
        }
    }
    impl Read for PipeReader {
        fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
            if buffer.is_empty() {
                return Ok(0);
            }
            // SAFETY: exclusive buffer borrow remains live through synchronous completion.
            unsafe { transfer(&self.0, buffer.as_mut_ptr(), buffer.len(), false) }
        }
    }
    impl Write for PipeWriter {
        fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
            if buffer.is_empty() {
                return Ok(0);
            }
            // SAFETY: WriteFile only reads this buffer; the borrow lasts through completion.
            unsafe { transfer(&self.0, buffer.as_ptr().cast_mut(), buffer.len(), true) }
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }
    impl Read for Pipe {
        fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
            self.reader.read(buffer)
        }
    }
    impl Write for Pipe {
        fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
            self.writer.write(buffer)
        }
        fn flush(&mut self) -> io::Result<()> {
            self.writer.flush()
        }
    }
    pub fn creation(pid: u32) -> Option<u64> {
        // SAFETY: handles and output pointers are valid for each synchronous API call.
        unsafe {
            let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
            if process.is_null() {
                return None;
            }
            let mut times = [FILETIME {
                dwLowDateTime: 0,
                dwHighDateTime: 0,
            }; 4];
            let [created, exited, kernel, user] = &mut times;
            let ok = GetProcessTimes(process, created, exited, kernel, user);
            CloseHandle(process);
            (ok != 0 && times[1].dwLowDateTime == 0 && times[1].dwHighDateTime == 0).then(|| {
                (u64::from(times[0].dwHighDateTime) << 32) | u64::from(times[0].dwLowDateTime)
            })
        }
    }
    pub fn connect(record: &Record) -> Result<Pipe, String> {
        validate_process(record, creation)?;
        let path: Vec<u16> = record.pipe.encode_utf16().chain(Some(0)).collect();
        // SAFETY: NUL-terminated validated pipe path; OwnedHandle takes ownership once.
        let file = unsafe {
            let handle = CreateFileW(
                path.as_ptr(),
                GENERIC_READ | GENERIC_WRITE,
                0,
                std::ptr::null(),
                OPEN_EXISTING,
                FILE_FLAG_OVERLAPPED,
                std::ptr::null_mut(),
            );
            if handle == INVALID_HANDLE_VALUE {
                return Err(NOT_RUNNING.into());
            }
            OwnedHandle::from_raw_handle(handle)
        };
        let mut pid = 0;
        // SAFETY: the owned handle keeps the pipe alive and pid is writable.
        if unsafe { GetNamedPipeServerProcessId(file.as_raw_handle(), &mut pid) } == 0
            || pid != record.pid
        {
            return Err(NOT_RUNNING.into());
        }
        let handle = Arc::new(file);
        Ok(Pipe {
            reader: PipeReader(handle.clone()),
            writer: PipeWriter(handle),
        })
    }
    pub fn run() -> Result<i32, String> {
        let root = std::env::var_os("LOCALAPPDATA").ok_or(NOT_RUNNING)?;
        let path = std::path::PathBuf::from(root).join("Goodtype/helper/instance.json");
        let mut bytes = Vec::new();
        File::open(path)
            .and_then(|file| file.take(4097).read_to_end(&mut bytes))
            .map_err(|_| NOT_RUNNING)?;
        if bytes.len() > 4096 {
            return Err(NOT_RUNNING.into());
        }
        let record = parse_record(&bytes)?;
        let mut pipe = connect(&record)?;
        let token = launch_token(&std::env::args().skip(1).collect::<Vec<_>>())?;
        handshake_launch(&mut pipe, &record, token.as_deref())?;
        let (reader, writer) = pipe.split();
        let (send, recv) = std::sync::mpsc::channel();
        let send2 = send.clone();
        std::thread::spawn(move || {
            let _ = send.send(pump(io::stdin().lock(), writer, LINE_LIMIT));
        });
        std::thread::spawn(move || {
            let _ = send2.send(pump(reader, io::stdout().lock(), LINE_LIMIT));
        });
        match recv.recv().map_err(|e| e.to_string())? {
            Err(e) if e.kind() == io::ErrorKind::InvalidData => {
                eprintln!("{e}");
                Ok(3)
            }
            Err(e)
                if !matches!(
                    e.kind(),
                    io::ErrorKind::BrokenPipe
                        | io::ErrorKind::UnexpectedEof
                        | io::ErrorKind::ConnectionReset
                ) =>
            {
                Err(e.to_string())
            }
            _ => Ok(0),
        }
        // main exits immediately, closing every pipe reference so stdin EOF ends the
        // app session. Never join the other thread blocked on stdin or pipe input.
    }
}
fn main() {
    #[cfg(windows)]
    let code = match windows::run() {
        Ok(code) => code,
        Err(message) => {
            eprintln!("{message}");
            2
        }
    };
    #[cfg(not(windows))]
    let code = {
        eprintln!("Goodtype MCP relay is Windows only.");
        2
    };
    std::process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(windows)]
    #[test]
    fn real_named_pipe_welcome_and_echo() {
        use std::{
            fs::File,
            os::windows::io::{AsRawHandle, FromRawHandle},
        };
        use windows_sys::Win32::{
            Foundation::{ERROR_PIPE_CONNECTED, GetLastError, INVALID_HANDLE_VALUE},
            Storage::FileSystem::PIPE_ACCESS_DUPLEX,
            System::Pipes::{ConnectNamedPipe, CreateNamedPipeW, PIPE_TYPE_BYTE, PIPE_WAIT},
        };
        let mut record = record();
        record.pid = std::process::id();
        record.started = windows::creation(record.pid).unwrap();
        record.pipe = format!(r"\\.\pipe\goodtype-relay-test-{}", record.pid);
        let name: Vec<u16> = record.pipe.encode_utf16().chain(Some(0)).collect();
        // SAFETY: valid NUL-terminated name and synchronous pipe; File owns the handle.
        let mut server = unsafe {
            let handle = CreateNamedPipeW(
                name.as_ptr(),
                PIPE_ACCESS_DUPLEX,
                PIPE_TYPE_BYTE | PIPE_WAIT,
                1,
                8192,
                8192,
                0,
                std::ptr::null(),
            );
            assert_ne!(handle, INVALID_HANDLE_VALUE);
            File::from_raw_handle(handle)
        };
        let thread = std::thread::spawn(move || {
            // SAFETY: live server handle, no overlapped I/O.
            unsafe {
                let connected = ConnectNamedPipe(server.as_raw_handle(), std::ptr::null_mut());
                assert!(connected != 0 || GetLastError() == ERROR_PIPE_CONNECTED);
            }
            let mut hello = Vec::new();
            loop {
                let mut b = [0];
                server.read_exact(&mut b).unwrap();
                hello.push(b[0]);
                if b[0] == b'\n' {
                    break;
                }
            }
            let hello: serde_json::Value = serde_json::from_slice(&hello).unwrap();
            assert_eq!(hello["type"], "goodtype-relay-hello");
            server
                .write_all(b"{\"type\":\"goodtype-relay-welcome\",\"protocol\":1}\n")
                .unwrap();
            let mut echo = [0; 5];
            server.read_exact(&mut echo).unwrap();
            server.write_all(&echo).unwrap();
        });
        let mut client = windows::connect(&record).unwrap();
        handshake(&mut client, &record).unwrap();
        client.write_all(b"echo\n").unwrap();
        let mut echo = [0; 5];
        client.read_exact(&mut echo).unwrap();
        assert_eq!(&echo, b"echo\n");
        thread.join().unwrap();
    }
    #[cfg(windows)]
    #[test]
    fn real_named_pipe_reader_pending_two_requests() {
        // Run the potentially deadlocking exchange in a child, so timeout can close
        // all handles and threads rather than leaving a hung test process behind.
        const CHILD: &str = "GOODTYPE_RELAY_DUPLEX_TEST";
        if std::env::var_os(CHILD).is_none() {
            let mut child = std::process::Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "tests::real_named_pipe_reader_pending_two_requests",
                    "--nocapture",
                ])
                .env(CHILD, "1")
                .spawn()
                .unwrap();
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
            loop {
                if let Some(status) = child.try_wait().unwrap() {
                    assert!(status.success(), "duplex exchange failed: {status}");
                    return;
                }
                if std::time::Instant::now() >= deadline {
                    child.kill().unwrap();
                    child.wait().unwrap();
                    panic!("duplex exchange timed out: reader blocked concurrent writes");
                }
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
        }
        use std::{
            fs::File,
            os::windows::io::{AsRawHandle, FromRawHandle},
        };
        use windows_sys::Win32::{
            Foundation::{ERROR_PIPE_CONNECTED, GetLastError, INVALID_HANDLE_VALUE},
            Storage::FileSystem::PIPE_ACCESS_DUPLEX,
            System::Pipes::{ConnectNamedPipe, CreateNamedPipeW, PIPE_TYPE_BYTE, PIPE_WAIT},
        };
        let mut record = record();
        record.pid = std::process::id();
        record.started = windows::creation(record.pid).unwrap();
        record.pipe = format!(r"\\.\pipe\goodtype-relay-duplex-test-{}", record.pid);
        let name: Vec<u16> = record.pipe.encode_utf16().chain(Some(0)).collect();
        // SAFETY: valid NUL-terminated name and synchronous pipe; File owns the handle.
        let mut server = unsafe {
            let handle = CreateNamedPipeW(
                name.as_ptr(),
                PIPE_ACCESS_DUPLEX,
                PIPE_TYPE_BYTE | PIPE_WAIT,
                1,
                8192,
                8192,
                0,
                std::ptr::null(),
            );
            assert_ne!(handle, INVALID_HANDLE_VALUE);
            File::from_raw_handle(handle)
        };
        let thread = std::thread::spawn(move || {
            // SAFETY: live server handle, no overlapped I/O.
            unsafe {
                let connected = ConnectNamedPipe(server.as_raw_handle(), std::ptr::null_mut());
                assert!(connected != 0 || GetLastError() == ERROR_PIPE_CONNECTED);
            }
            let mut hello = Vec::new();
            loop {
                let mut b = [0];
                server.read_exact(&mut b).unwrap();
                hello.push(b[0]);
                if b[0] == b'\n' {
                    break;
                }
            }
            let hello: serde_json::Value = serde_json::from_slice(&hello).unwrap();
            assert_eq!(hello["type"], "goodtype-relay-hello");
            server
                .write_all(b"{\"type\":\"goodtype-relay-welcome\",\"protocol\":1}\n")
                .unwrap();
            // No response until BOTH requests arrive.
            let mut requests = [0; 13];
            server.read_exact(&mut requests).unwrap();
            assert_eq!(&requests, b"first\nsecond\n");
            server.write_all(b"reply1\nreply2\n").unwrap();
        });
        let mut client = windows::connect(&record).unwrap();
        handshake(&mut client, &record).unwrap();
        let (mut reader, mut writer) = client.split();
        windows::READ_PENDING.store(false, std::sync::atomic::Ordering::SeqCst);
        let read_thread = std::thread::spawn(move || {
            let mut replies = [0; 14];
            reader.read_exact(&mut replies).unwrap();
            assert_eq!(&replies, b"reply1\nreply2\n");
            assert_eq!(reader.read(&mut [0]).unwrap(), 0);
        });
        // ReadFile has actually returned ERROR_IO_PENDING: the server has no data,
        // and the read must remain outstanding while both writes complete.
        while !windows::READ_PENDING.load(std::sync::atomic::Ordering::SeqCst) {
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        writer.write_all(b"first\n").unwrap();
        writer.write_all(b"second\n").unwrap();
        read_thread.join().unwrap();
        thread.join().unwrap();
        assert_eq!(
            writer.write(b"closed\n").unwrap_err().kind(),
            io::ErrorKind::BrokenPipe
        );
    }
    use std::io::Cursor;
    fn record_json() -> serde_json::Value {
        serde_json::json!({"protocol":1,"instanceId":"a".repeat(32),"pipe":format!(r"\\.\pipe\goodtype-helper-{}", "a".repeat(32)),"token":"b".repeat(64),"pid":1234,"startedAt":123456})
    }
    fn record() -> Record {
        parse_record(&serde_json::to_vec(&record_json()).unwrap()).unwrap()
    }
    struct Stream {
        input: Cursor<Vec<u8>>,
        output: Vec<u8>,
    }
    impl Read for Stream {
        fn read(&mut self, b: &mut [u8]) -> io::Result<usize> {
            self.input.read(b)
        }
    }
    impl Write for Stream {
        fn write(&mut self, b: &[u8]) -> io::Result<usize> {
            self.output.write(b)
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }
    fn stream(bytes: &[u8]) -> Stream {
        Stream {
            input: Cursor::new(bytes.to_vec()),
            output: vec![],
        }
    }
    #[test]
    fn launch_token_parse_and_hello() {
        let token = "a".repeat(64);
        assert_eq!(launch_token(&[]).unwrap(), None);
        assert_eq!(
            launch_token(&["--launch-token".into(), token.clone()]).unwrap(),
            Some(token.clone())
        );
        for args in [
            vec!["--launch-token".into()],
            vec!["--launch-token".into(), "x".repeat(64)],
            vec!["--other".into(), token.clone()],
        ] {
            assert!(launch_token(&args).is_err());
        }
        let mut s = stream(b"{\"type\":\"goodtype-relay-welcome\",\"protocol\":1}\n");
        handshake_launch(&mut s, &record(), Some(&token)).unwrap();
        let hello: serde_json::Value = serde_json::from_slice(&s.output).unwrap();
        assert_eq!(hello["launchToken"], token);
    }
    #[test]
    fn record_valid_and_extra_fields() {
        let mut value = record_json();
        value["future"] = true.into();
        let parsed = parse_record(&serde_json::to_vec(&value).unwrap()).unwrap();
        assert_eq!(parsed.pid, 1234);
        assert_eq!(parsed.started, 123456);
    }
    #[test]
    fn record_missing_and_malformed() {
        assert!(parse_record(b"{").is_err());
        for key in [
            "protocol",
            "instanceId",
            "pipe",
            "token",
            "pid",
            "startedAt",
        ] {
            let mut v = record_json();
            v.as_object_mut().unwrap().remove(key);
            assert!(
                parse_record(&serde_json::to_vec(&v).unwrap()).is_err(),
                "{key}"
            );
            let mut v = record_json();
            v[key] = "invalid".into();
            assert!(
                parse_record(&serde_json::to_vec(&v).unwrap()).is_err(),
                "{key}"
            );
        }
    }
    #[test]
    fn stale_record_dead_pid_and_reused_pid() {
        assert_eq!(
            validate_process(&record(), |_| None),
            Err(NOT_RUNNING.into())
        );
        assert!(validate_process(&record(), |_| Some(123)).is_err());
        assert!(validate_process(&record(), |_| Some(123456)).is_ok());
    }
    #[cfg(windows)]
    #[test]
    fn stale_record_real_missing_pid() {
        assert!(validate_process(&record(), |_| windows::creation(u32::MAX)).is_err());
    }
    #[test]
    fn handshake_welcome_preserves_following_bytes() {
        let mut s = stream(b"{\"type\":\"goodtype-relay-welcome\",\"protocol\":1}\nMCP");
        handshake(&mut s, &record()).unwrap();
        let mut rest = String::new();
        s.read_to_string(&mut rest).unwrap();
        assert_eq!(rest, "MCP");
        let hello: serde_json::Value = serde_json::from_slice(&s.output).unwrap();
        assert_eq!(hello["token"], "b".repeat(64));
        assert_eq!(hello["type"], "goodtype-relay-hello");
    }
    #[test]
    fn handshake_each_refusal() {
        for reason in ["bad_token", "protocol_mismatch", "disabled", "busy"] {
            let mut bytes = serde_json::to_vec(&serde_json::json!({"type":"goodtype-relay-refused","reason":reason,"message":format!("refused: {reason}")})).unwrap();
            bytes.push(b'\n');
            assert_eq!(
                handshake(&mut stream(&bytes), &record()),
                Err(format!("refused: {reason}"))
            );
        }
    }
    #[test]
    fn handshake_oversized_and_malformed() {
        assert!(
            handshake(&mut stream(&vec![b'x'; 4097]), &record())
                .unwrap_err()
                .contains("4096")
        );
        for bytes in [
            b"oops\n".as_slice(),
            b"{}\n",
            b"{\"type\":\"goodtype-relay-welcome\",\"protocol\":2}\n",
            b"",
        ] {
            assert!(handshake(&mut stream(bytes), &record()).is_err());
        }
    }
    #[test]
    fn pump_verbatim_partial_lines_and_binary_utf8() {
        let bytes = b"first\n\0\xff\xc3\xa4\npartial";
        let mut output = vec![];
        pump(bytes.as_slice(), &mut output, 32).unwrap();
        assert_eq!(output, bytes);
    }
    #[test]
    fn pump_bound_both_directions_and_chunk_boundaries() {
        for _direction in ["stdin-to-pipe", "pipe-to-stdout"] {
            assert_eq!(
                pump(b"12345".as_slice(), io::sink(), 4).unwrap_err().kind(),
                io::ErrorKind::InvalidData
            );
            pump(b"1234\n1234".as_slice(), io::sink(), 4).unwrap();
            assert!(pump(vec![b'x'; 8193].as_slice(), io::sink(), 8192).is_err());
        }
    }
}
