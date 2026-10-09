use crate::error::{AppError, AppResult};
use std::{
    cell::RefCell,
    collections::HashMap,
    io::{Read, Write},
    process::{Command, Output, Stdio},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex, OnceLock,
    },
    time::{Duration, Instant},
};

type Registry = Mutex<HashMap<String, (Arc<AtomicBool>, bool)>>;
fn registry() -> &'static Registry {
    static TASKS: OnceLock<Registry> = OnceLock::new();
    TASKS.get_or_init(Default::default)
}
thread_local! { static CURRENT: RefCell<Option<(String, Arc<AtomicBool>)>> = const { RefCell::new(None) }; }

pub struct Session {
    id: String,
    previous: Option<(String, Arc<AtomicBool>)>,
}
impl Session {
    pub fn begin(id: Option<String>) -> AppResult<Self> {
        let id = id.unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
        crate::store::validate_id(&id)?;
        let mut tasks = registry()
            .lock()
            .map_err(|_| AppError::Io("任务状态不可用".into()))?;
        let entry = tasks
            .entry(id.clone())
            .or_insert_with(|| (Arc::new(AtomicBool::new(false)), false));
        if entry.1 {
            return Err(AppError::Invalid("同一任务已在运行".into()));
        }
        entry.1 = true;
        let token = entry.0.clone();
        let previous = CURRENT.with(|c| c.replace(Some((id.clone(), token))));
        Ok(Self { id, previous })
    }
}
impl Drop for Session {
    fn drop(&mut self) {
        CURRENT.with(|c| c.replace(self.previous.take()));
        if let Ok(mut tasks) = registry().lock() {
            tasks.remove(&self.id);
        }
    }
}
pub fn id() -> String {
    CURRENT.with(|c| c.borrow().as_ref().map(|c| c.0.clone()).unwrap_or_default())
}
pub fn check() -> AppResult<()> {
    if CURRENT.with(|c| {
        c.borrow()
            .as_ref()
            .is_some_and(|c| c.1.load(Ordering::Relaxed))
    }) {
        Err(AppError::Invalid("任务已取消".into()))
    } else {
        Ok(())
    }
}
#[tauri::command]
pub fn cancel_task(task_id: String) -> AppResult<()> {
    crate::store::validate_id(&task_id)?;
    let mut tasks = registry()
        .lock()
        .map_err(|_| AppError::Io("任务状态不可用".into()))?;
    // Cancellation can arrive while the IPC job is still queued.
    if tasks.len() > 256 && !tasks.contains_key(&task_id) {
        return Err(AppError::Invalid("未知任务".into()));
    }
    tasks
        .entry(task_id)
        .or_insert_with(|| (Arc::new(AtomicBool::new(false)), false))
        .0
        .store(true, Ordering::Relaxed);
    Ok(())
}

/// Drain both pipes concurrently so large output cannot deadlock; always reap on failure.
pub fn output(cmd: &mut Command, timeout: Duration, input: Option<&[u8]>) -> AppResult<Output> {
    check()?;
    cmd.stdin(if input.is_some() {
        Stdio::piped()
    } else {
        Stdio::null()
    })
    .stdout(Stdio::piped())
    .stderr(Stdio::piped());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x08000000);
    }
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        cmd.process_group(0);
    }
    let mut child = cmd.spawn()?;
    let tree = match ProcessTree::attach(&child) {
        Ok(tree) => tree,
        Err(e) => {
            let _ = child.kill();
            let _ = child.wait();
            return Err(e);
        }
    };
    let mut stdout = child
        .stdout
        .take()
        .ok_or_else(|| AppError::Io("子进程输出不可用".into()))?;
    let mut stderr = child
        .stderr
        .take()
        .ok_or_else(|| AppError::Io("子进程错误输出不可用".into()))?;
    let out = std::thread::spawn(move || {
        let mut data = vec![];
        stdout.read_to_end(&mut data).map(|_| data)
    });
    let err = std::thread::spawn(move || {
        let mut data = vec![];
        stderr.read_to_end(&mut data).map(|_| data)
    });
    let start = Instant::now();
    // Input must also participate in the deadline: a child can stop reading stdin.
    let writer = input.map(|bytes| {
        let bytes = bytes.to_vec();
        let mut stdin = child.stdin.take().expect("piped stdin");
        std::thread::spawn(move || stdin.write_all(&bytes))
    });
    let result = (|| loop {
        check()?;
        if let Some(status) = child.try_wait()? {
            return Ok(status);
        }
        if start.elapsed() >= timeout {
            return Err(AppError::Io(format!(
                "子进程超过 {} 秒，已终止",
                timeout.as_secs()
            )));
        }
        std::thread::sleep(Duration::from_millis(50));
    })();
    // Descendants may keep inherited pipes open after the foreground command exits.
    drop(tree);
    if result.is_err() {
        let _ = child.kill();
        let _ = child.wait();
    }
    let stdout = out
        .join()
        .map_err(|_| AppError::Io("读取子进程输出失败".into()));
    let stderr = err
        .join()
        .map_err(|_| AppError::Io("读取子进程错误输出失败".into()));
    let written = writer
        .map(|w| {
            w.join()
                .map_err(|_| AppError::Io("写入子进程输入失败".into()))
        })
        .transpose();
    let status = result?; // Preserve the cancellation/timeout reason over pipe errors.
    if let Some(result) = written? {
        result?;
    }
    Ok(Output {
        status,
        stdout: stdout??,
        stderr: stderr??,
    })
}

#[cfg(windows)]
struct ProcessTree(windows_sys::Win32::Foundation::HANDLE);
#[cfg(windows)]
impl ProcessTree {
    fn attach(child: &std::process::Child) -> AppResult<Self> {
        use std::os::windows::io::AsRawHandle;
        use windows_sys::Win32::System::JobObjects::*;
        // A private job owns this subprocess tree; closing it cannot affect other jobs.
        let handle = unsafe { CreateJobObjectW(std::ptr::null(), std::ptr::null()) };
        if handle.is_null() {
            return Err(std::io::Error::last_os_error().into());
        }
        let tree = Self(handle);
        let mut limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
        limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        let ok = unsafe {
            SetInformationJobObject(
                handle,
                JobObjectExtendedLimitInformation,
                &limits as *const _ as *const _,
                std::mem::size_of_val(&limits) as u32,
            ) != 0
                && AssignProcessToJobObject(handle, child.as_raw_handle()) != 0
        };
        if !ok {
            return Err(std::io::Error::last_os_error().into());
        }
        Ok(tree)
    }
}
#[cfg(windows)]
impl Drop for ProcessTree {
    fn drop(&mut self) {
        unsafe {
            windows_sys::Win32::Foundation::CloseHandle(self.0);
        }
    }
}
#[cfg(unix)]
struct ProcessTree(i32);
#[cfg(unix)]
impl ProcessTree {
    fn attach(child: &std::process::Child) -> AppResult<Self> {
        Ok(Self(child.id() as i32))
    }
}
#[cfg(unix)]
impl Drop for ProcessTree {
    fn drop(&mut self) {
        unsafe {
            libc::kill(-self.0, libc::SIGKILL);
        }
    }
}
