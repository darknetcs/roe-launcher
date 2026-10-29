//! Spawns child processes so they can be stopped as a whole (Proton/Wine and the server spawn many
//! children), and streams their output to a log sink.
//! Unix: an own process group, SIGTERM then SIGKILL. Windows: a job object, no console window.
//! Processes started with `spawn_owned` (the server, the LAN bridge) must not outlive the launcher: Windows ends
//! them with the launcher's last handle (kill-on-close job); on Unix `stop_owned_on_signals` stops them on
//! SIGTERM/SIGINT/SIGHUP, and `track_owned` lists them in a file so the next start stops any a SIGKILL or crash left.
use crate::Sink;
use anyhow::{Context, Result};
use std::io::{BufRead, BufReader, Read};
use std::process::{Command, Stdio};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

const NO_CODE: i64 = i64::MIN;

/// Running owned processes (process-group leaders) with their start times (Linux /proc; 0 elsewhere).
static OWNED: Mutex<Vec<(u32, u64)>> = Mutex::new(Vec::new());
/// Where `OWNED` is written for the next launcher start (set by `track_owned`).
static OWNED_FILE: OnceLock<PathBuf> = OnceLock::new();

fn owned_changed(f: impl FnOnce(&mut Vec<(u32, u64)>)) {
    let Ok(mut list) = OWNED.lock() else { return };
    f(&mut list);
    if let Some(path) = OWNED_FILE.get() {
        let text: String = list.iter().map(|(pid, start)| format!("{pid}\t{start}\n")).collect();
        let _ = if text.is_empty() { std::fs::remove_file(path).or(Ok(())) } else { std::fs::write(path, text) };
    }
}

/// Keeps the owned-process list in `file` from now on, after stopping what an earlier launcher left in it
/// (killed with SIGKILL or crashed). Returns how many leftover processes were stopped.
pub fn track_owned(file: PathBuf) -> usize {
    let stopped = stop_leftovers(&file);
    let _ = OWNED_FILE.set(file);
    owned_changed(|_| {});
    stopped
}

/// Stops the process groups listed in `file` that still run, recognised by PID and start time (a PID the system
/// gave to another program since then is left alone).
fn stop_leftovers(file: &Path) -> usize {
    let Ok(text) = std::fs::read_to_string(file) else { return 0 };
    let _ = std::fs::remove_file(file);
    #[cfg(unix)]
    {
        let groups: Vec<u32> = text
            .lines()
            .filter_map(|l| {
                let (pid, start) = l.split_once('\t')?;
                let (pid, start) = (pid.trim().parse().ok()?, start.trim().parse().ok()?);
                (start != 0 && unix::start_time(pid) == Some(start)).then_some(pid)
            })
            .collect();
        unix::stop_groups(&groups, Duration::from_secs(5));
        groups.len()
    }
    #[cfg(not(unix))]
    {
        let _ = text; // Windows: the kill-on-close job already ended them with the launcher
        0
    }
}

/// Stops every owned process now and waits for them (up to `grace`, then forced). For the signal handler and a
/// crash; a normal quit stops the server itself.
pub fn stop_owned(grace: Duration) {
    let groups: Vec<u32> = OWNED.lock().map(|l| l.iter().map(|(pid, _)| *pid).collect()).unwrap_or_default();
    #[cfg(unix)]
    unix::stop_groups(&groups, grace);
    #[cfg(not(unix))]
    let _ = (groups, grace);
}

/// Unix: on SIGTERM, SIGINT or SIGHUP stop the owned processes, then exit (128 + signal, like a shell).
/// Windows needs nothing: the kill-on-close job ends them however the launcher ends.
pub fn stop_owned_on_signals(on_exit: impl Fn(i32) + Send + 'static) {
    #[cfg(unix)]
    unix::catch_signals(on_exit);
    #[cfg(not(unix))]
    let _ = on_exit;
}

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
    use std::sync::atomic::{AtomicI32, Ordering};
    use std::time::{Duration, Instant};

    pub fn signal_group(pid: u32, sig: i32) {
        // SAFETY: plain kill(2) on the process group we created with process_group(0).
        unsafe {
            libc::kill(-(pid as i32), sig);
        }
    }

    fn group_alive(pid: u32) -> bool {
        // SAFETY: signal 0 only checks that the group exists.
        unsafe { libc::kill(-(pid as i32), 0) == 0 }
    }

    /// SIGTERM to each group, SIGKILL to those still there after `grace`.
    pub fn stop_groups(groups: &[u32], grace: Duration) {
        for &g in groups {
            signal_group(g, libc::SIGTERM);
        }
        let t = Instant::now();
        while groups.iter().any(|&g| group_alive(g)) && t.elapsed() < grace {
            std::thread::sleep(Duration::from_millis(100));
        }
        for &g in groups.iter().filter(|&&g| group_alive(g)) {
            signal_group(g, libc::SIGKILL);
        }
    }

    /// Start time of a process in clock ticks since boot (field 22 of /proc/PID/stat); None if it is gone.
    #[cfg(target_os = "linux")]
    pub fn start_time(pid: u32) -> Option<u64> {
        let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
        // the name (field 2) is in parentheses and may contain spaces: count from the last ')'
        stat.rsplit_once(')')?.1.split_whitespace().nth(19)?.parse().ok()
    }

    #[cfg(not(target_os = "linux"))]
    pub fn start_time(_pid: u32) -> Option<u64> {
        None
    }

    static CAUGHT: AtomicI32 = AtomicI32::new(0);

    extern "C" fn on_signal(sig: libc::c_int) {
        // async-signal-safe: only an atomic store; the watcher thread does the work
        CAUGHT.store(sig, Ordering::SeqCst);
    }

    pub fn catch_signals(on_exit: impl Fn(i32) + Send + 'static) {
        for sig in [libc::SIGTERM, libc::SIGINT, libc::SIGHUP] {
            // SAFETY: installs a handler that only stores to an atomic.
            unsafe {
                libc::signal(sig, on_signal as extern "C" fn(libc::c_int) as libc::sighandler_t);
            }
        }
        std::thread::spawn(move || loop {
            std::thread::sleep(Duration::from_millis(100));
            let sig = CAUGHT.load(Ordering::SeqCst);
            if sig != 0 {
                on_exit(sig);
                super::stop_owned(Duration::from_secs(5));
                std::process::exit(128 + sig);
            }
        });
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
    use windows_sys::Win32::System::JobObjects::{
        AssignProcessToJobObject, CreateJobObjectW, JobObjectExtendedLimitInformation, SetInformationJobObject, TerminateJobObject,
        JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
    };
    use windows_sys::Win32::System::Threading::{OpenProcess, PROCESS_SET_QUOTA, PROCESS_TERMINATE};

    pub const CREATE_NO_WINDOW: u32 = 0x0800_0000;

    /// A job object holding a child and everything it starts. "Kill on close" only for owned processes
    /// (`spawn_owned`): closing the launcher must not take the game down with it (same as on Linux).
    pub struct Job(usize);

    impl Job {
        /// Puts the process into a new job. Children it starts after this are in the job too;
        /// one started in the first instant before this call would not be (Python needs far longer
        /// than that to start anything).
        pub fn for_process(pid: u32, kill_on_close: bool) -> Option<Job> {
            // SAFETY: plain Win32 calls; every handle is checked and closed.
            unsafe {
                let job = CreateJobObjectW(std::ptr::null(), std::ptr::null());
                if job.is_null() {
                    return None;
                }
                if kill_on_close {
                    let mut info: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = std::mem::zeroed();
                    info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
                    let size = std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32;
                    if SetInformationJobObject(job, JobObjectExtendedLimitInformation, &info as *const _ as *const _, size) == 0 {
                        CloseHandle(job);
                        return None;
                    }
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

/// Starts a process that may outlive the launcher (the game).
pub fn spawn_logged(cmd: Command, sink: Sink) -> Result<Handle> {
    spawn(cmd, sink, false)
}

/// Starts a process that ends with the launcher, however the launcher ends (the server, the LAN bridge).
/// Keep the returned handle (or a clone) while it runs: on Windows dropping the last one ends the process.
pub fn spawn_owned(cmd: Command, sink: Sink) -> Result<Handle> {
    spawn(cmd, sink, true)
}

fn spawn(mut cmd: Command, sink: Sink, owned: bool) -> Result<Handle> {
    cmd.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
    #[cfg(unix)]
    std::os::unix::process::CommandExt::process_group(&mut cmd, 0);
    hide_window(&mut cmd);
    let mut child = cmd.spawn().with_context(|| format!("starting {:?}", cmd.get_program()))?;
    let pid = child.id();
    #[cfg(windows)]
    let job = match win::Job::for_process(pid, owned) {
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
    if owned {
        #[cfg(unix)]
        let start = unix::start_time(pid).unwrap_or(0);
        #[cfg(not(unix))]
        let start = 0;
        owned_changed(|l| l.push((pid, start)));
    }
    std::thread::spawn(move || {
        let status = child.wait();
        if owned {
            owned_changed(|l| l.retain(|(p, _)| *p != pid));
        }
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

    /// A process group not started by `spawn`, reaped in the background like a real orphan's parent would.
    fn group_of_its_own() -> u32 {
        let mut c = Command::new("sleep");
        c.arg("60");
        std::os::unix::process::CommandExt::process_group(&mut c, 0);
        let mut child = c.spawn().unwrap();
        let pid = child.id();
        std::thread::spawn(move || child.wait());
        pid
    }

    fn gone(pid: u32) -> bool {
        let t = Instant::now();
        while unix::start_time(pid).is_some() && t.elapsed() < Duration::from_secs(10) {
            std::thread::sleep(Duration::from_millis(20));
        }
        unix::start_time(pid).is_none()
    }

    #[test]
    #[cfg(target_os = "linux")]
    fn owned_processes_are_listed_and_stopped_with_the_launcher() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("owned.txt");
        assert_eq!(track_owned(file.clone()), 0);
        let (sink, _l) = collector();
        let mut c = Command::new("sh");
        c.args(["-c", "sleep 60 & sleep 60"]);
        let h = spawn_owned(c, sink).unwrap();
        let listed = std::fs::read_to_string(&file).unwrap();
        assert!(listed.starts_with(&format!("{}\t", h.pid)), "{listed}");
        stop_owned(Duration::from_secs(2));
        wait(&h);
        assert!(!h.is_running());
        std::thread::sleep(Duration::from_millis(100));
        assert!(!file.exists(), "the list is removed once nothing owned runs");
    }

    #[test]
    #[cfg(target_os = "linux")]
    fn leftovers_are_stopped_but_a_reused_pid_is_not() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("owned.txt");
        let (left, other) = (group_of_its_own(), group_of_its_own());
        std::thread::sleep(Duration::from_millis(100));
        let start = unix::start_time(left).unwrap();
        // `other` stands for a PID the system gave to another program: same PID, different start time
        std::fs::write(&file, format!("{left}\t{start}\n{other}\t{}\n", unix::start_time(other).unwrap() + 1)).unwrap();
        assert_eq!(stop_leftovers(&file), 1);
        assert!(gone(left));
        assert!(unix::start_time(other).is_some(), "an unrelated process must be left alone");
        assert!(!file.exists());
        unix::signal_group(other, libc::SIGKILL);
    }
}

#[cfg(all(test, windows))]
mod windows_tests {
    use super::*;

    #[test]
    fn an_owned_process_ends_with_the_launchers_last_handle() {
        let sink: Sink = Arc::new(|_| {});
        let mut c = Command::new("ping");
        c.args(["-n", "60", "127.0.0.1"]);
        let h = spawn_owned(c, sink).unwrap();
        let exited = h.exited.clone();
        std::thread::sleep(Duration::from_millis(300));
        assert!(!exited.load(Ordering::SeqCst));
        drop(h); // like the launcher dying: the job's handle closes
        let t = Instant::now();
        while !exited.load(Ordering::SeqCst) && t.elapsed() < Duration::from_secs(10) {
            std::thread::sleep(Duration::from_millis(50));
        }
        assert!(exited.load(Ordering::SeqCst));
    }
}
