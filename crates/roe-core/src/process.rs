//! Spawns child processes so they can be stopped as a whole (Proton/Wine and the server spawn many
//! children), and streams their output to a log sink.
//! Unix: an own process group, SIGTERM then SIGKILL. Windows: a job object, no console window.
use crate::Sink;
use anyhow::{Context, Result};
use std::io::{BufRead, BufReader, Read};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

const NO_CODE: i64 = i64::MIN;

#[derive(Clone)]
pub struct Handle {
    pub pid: u32,
    pub started: Instant,
    exited: Arc<AtomicBool>,
    code: Arc<AtomicI64>,
    /// Set by `request_stop`, so an exit we caused is not reported as a crash.
    stopped: Arc<AtomicBool>,
    #[cfg(windows)]
    job: Arc<win::Job>,
}

impl Handle {
    pub fn is_running(&self) -> bool {
        !self.exited.load(Ordering::SeqCst)
    }

    /// Exit code (negative = killed by that signal on Unix); None while running.
    pub fn exit_code(&self) -> Option<i64> {
        let c = self.code.load(Ordering::SeqCst);
        (self.exited.load(Ordering::SeqCst) && c != NO_CODE).then_some(c)
    }

    /// True when `request_stop` was called: the exit was asked for, not a crash.
    pub fn stop_requested(&self) -> bool {
        self.stopped.load(Ordering::SeqCst)
    }

    /// Asks the whole group to stop, then forces it after `grace`. Non-blocking.
    /// Windows has no polite signal for windowless processes, so there it ends the job at once.
    pub fn request_stop(&self, grace: Duration) {
        if !self.is_running() {
            return;
        }
        self.stopped.store(true, Ordering::SeqCst);
        let h = self.clone();
        std::thread::spawn(move || {
            #[cfg(unix)]
            {
                unix::signal_group(h.pid, libc::SIGTERM);
                let t = Instant::now();
                while h.is_running() && t.elapsed() < grace {
                    std::thread::sleep(Duration::from_millis(100));
                }
                if h.is_running() {
                    unix::signal_group(h.pid, libc::SIGKILL);
                }
            }
            #[cfg(windows)]
            {
                let _ = grace;
                h.job.terminate();
            }
        });
    }
}

#[cfg(unix)]
mod unix {
    pub fn signal_group(pid: u32, sig: i32) {
        // SAFETY: plain kill(2) on the process group we created with process_group(0).
        unsafe {
            libc::kill(-(pid as i32), sig);
        }
    }
}

/// Keeps console programs (Python, pip, dotnet) from flashing a console window on Windows.
pub fn hide_window(cmd: &mut Command) -> &mut Command {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(win::CREATE_NO_WINDOW);
    }
    cmd
}

#[cfg(windows)]
mod win {
    use windows_sys::Win32::Foundation::{CloseHandle, HANDLE};
    use windows_sys::Win32::System::JobObjects::{AssignProcessToJobObject, CreateJobObjectW, TerminateJobObject};
    use windows_sys::Win32::System::Threading::{OpenProcess, PROCESS_SET_QUOTA, PROCESS_TERMINATE};

    pub const CREATE_NO_WINDOW: u32 = 0x0800_0000;

    /// A job object holding a child and everything it starts. Not "kill on close": closing the
    /// launcher must not take the game down with it (same as on Linux).
    pub struct Job(usize);

    impl Job {
        /// Puts the process into a new job. Children it starts after this are in the job too;
        /// one started in the first instant before this call would not be (Python needs far longer
        /// than that to start anything).
        pub fn for_process(pid: u32) -> Option<Job> {
            // SAFETY: plain Win32 calls; every handle is checked and closed.
            unsafe {
                let job = CreateJobObjectW(std::ptr::null(), std::ptr::null());
                if job.is_null() {
                    return None;
                }
                let process = OpenProcess(PROCESS_SET_QUOTA | PROCESS_TERMINATE, 0, pid);
                let ok = !process.is_null() && AssignProcessToJobObject(job, process) != 0;
                if !process.is_null() {
                    CloseHandle(process);
                }
                if !ok {
                    CloseHandle(job);
                    return None;
                }
                Some(Job(job as usize))
            }
        }

        pub fn terminate(&self) {
            // SAFETY: the handle stays valid until drop.
            unsafe {
                TerminateJobObject(self.0 as HANDLE, 1);
            }
        }
    }

    impl Drop for Job {
        fn drop(&mut self) {
            // SAFETY: closing our own handle once.
            unsafe {
                CloseHandle(self.0 as HANDLE);
            }
        }
    }
}

fn pump(reader: impl Read + Send + 'static, sink: Sink) {
    std::thread::spawn(move || {
        let mut r = BufReader::new(reader);
        let mut buf = Vec::new();
        loop {
            buf.clear();
            match r.read_until(b'\n', &mut buf) {
                Ok(0) | Err(_) => break,
                Ok(_) => {
                    let line = String::from_utf8_lossy(&buf).trim_end_matches(['\r', '\n']).to_string();
                    if !line.is_empty() {
                        sink(line);
                    }
                }
            }
        }
    });
}

pub fn spawn_logged(mut cmd: Command, sink: Sink) -> Result<Handle> {
    cmd.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
    #[cfg(unix)]
    std::os::unix::process::CommandExt::process_group(&mut cmd, 0);
    hide_window(&mut cmd);
    let mut child = cmd.spawn().with_context(|| format!("starting {:?}", cmd.get_program()))?;
    let pid = child.id();
    #[cfg(windows)]
    let job = match win::Job::for_process(pid) {
        Some(j) => Arc::new(j),
        None => {
            let _ = child.kill();
            anyhow::bail!("could not supervise {:?} (job object)", cmd.get_program());
        }
    };
    pump(child.stdout.take().unwrap(), sink.clone());
    pump(child.stderr.take().unwrap(), sink);
    let exited = Arc::new(AtomicBool::new(false));
    let code = Arc::new(AtomicI64::new(NO_CODE));
    let (e2, c2) = (exited.clone(), code.clone());
    std::thread::spawn(move || {
        let status = child.wait();
        let c = match status {
            Ok(s) => s.code().map(i64::from).or_else(|| signal_of(&s)).unwrap_or(-1),
            Err(_) => -1,
        };
        c2.store(c, Ordering::SeqCst);
        e2.store(true, Ordering::SeqCst);
    });
    Ok(Handle {
        pid,
        started: Instant::now(),
        exited,
        code,
        stopped: Arc::new(AtomicBool::new(false)),
        #[cfg(windows)]
        job,
    })
}

#[cfg(unix)]
fn signal_of(s: &std::process::ExitStatus) -> Option<i64> {
    std::os::unix::process::ExitStatusExt::signal(s).map(|n| -(n as i64))
}

#[cfg(not(unix))]
fn signal_of(_: &std::process::ExitStatus) -> Option<i64> {
    None
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::sync::Mutex;

    fn collector() -> (Sink, Arc<Mutex<Vec<String>>>) {
        let v = Arc::new(Mutex::new(Vec::new()));
        let v2 = v.clone();
        (Arc::new(move |l| v2.lock().unwrap().push(l)), v)
    }

    fn wait(h: &Handle) {
        let t = Instant::now();
        while h.is_running() && t.elapsed() < Duration::from_secs(10) {
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    #[test]
    fn streams_output_and_reports_exit_code() {
        let (sink, lines) = collector();
        let mut c = Command::new("sh");
        c.args(["-c", "echo out; echo err >&2; exit 3"]);
        let h = spawn_logged(c, sink).unwrap();
        wait(&h);
        std::thread::sleep(Duration::from_millis(100));
        assert_eq!(h.exit_code(), Some(3));
        let l = lines.lock().unwrap();
        assert!(l.contains(&"out".to_string()) && l.contains(&"err".to_string()));
    }

    #[test]
    fn stop_kills_the_whole_process_group() {
        let (sink, _l) = collector();
        let mut c = Command::new("sh");
        c.args(["-c", "sleep 60 & sleep 60"]); // a child plus a grandchild
        let h = spawn_logged(c, sink).unwrap();
        std::thread::sleep(Duration::from_millis(200));
        assert!(h.is_running());
        h.request_stop(Duration::from_secs(2));
        wait(&h);
        assert!(!h.is_running());
        assert_eq!(h.exit_code(), Some(-(libc::SIGTERM as i64)));
        assert!(h.stop_requested());
    }
}
