use std::fmt;

pub(crate) const MEMORY_BYTES: u64 = 4 * 1024 * 1024 * 1024;
pub(crate) const CPU_SECONDS: u64 = 1_800;
pub(crate) const MAX_PROCESSES: u32 = 8;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LimitBreach {
    #[cfg(any(windows, test))]
    Memory,
    Cpu,
    #[cfg(windows)]
    Processes,
}

impl fmt::Display for LimitBreach {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let resource = match self {
            #[cfg(any(windows, test))]
            Self::Memory => "memory",
            Self::Cpu => "CPU time",
            #[cfg(windows)]
            Self::Processes => "process count",
        };
        write!(f, "plugin exceeded {resource} limit")
    }
}
impl std::error::Error for LimitBreach {}

#[cfg(windows)]
mod platform {
    use super::*;
    use anyhow::{Context, Result};
    use std::{
        mem::size_of,
        os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle},
        sync::{
            atomic::{AtomicBool, Ordering},
            Arc,
        },
    };
    use tokio::{process::Child, sync::mpsc};
    use windows_sys::Win32::{
        Foundation::{HANDLE, INVALID_HANDLE_VALUE},
        System::{
            Diagnostics::ToolHelp::{
                CreateToolhelp32Snapshot, Thread32First, Thread32Next, TH32CS_SNAPTHREAD,
                THREADENTRY32,
            },
            JobObjects::{
                AssignProcessToJobObject, CreateJobObjectW,
                JobObjectAssociateCompletionPortInformation, JobObjectExtendedLimitInformation,
                SetInformationJobObject, TerminateJobObject, JOBOBJECT_ASSOCIATE_COMPLETION_PORT,
                JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JOB_OBJECT_LIMIT_ACTIVE_PROCESS,
                JOB_OBJECT_LIMIT_JOB_MEMORY, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
                JOB_OBJECT_LIMIT_PROCESS_TIME,
            },
            SystemServices::{
                JOB_OBJECT_MSG_ACTIVE_PROCESS_LIMIT, JOB_OBJECT_MSG_END_OF_PROCESS_TIME,
                JOB_OBJECT_MSG_JOB_MEMORY_LIMIT, JOB_OBJECT_MSG_PROCESS_MEMORY_LIMIT,
            },
            Threading::{
                OpenProcess, OpenThread, ResumeThread, PROCESS_SET_QUOTA, PROCESS_TERMINATE,
                THREAD_SUSPEND_RESUME,
            },
            IO::{CreateIoCompletionPort, GetQueuedCompletionStatus},
        },
    };

    fn handle(raw: HANDLE) -> Result<OwnedHandle> {
        anyhow::ensure!(
            !raw.is_null() && raw != INVALID_HANDLE_VALUE,
            "Windows handle unavailable: {}",
            std::io::Error::last_os_error()
        );
        Ok(unsafe { OwnedHandle::from_raw_handle(raw) })
    }

    pub(crate) struct Limits {
        job: Arc<OwnedHandle>,
        stopped: Arc<AtomicBool>,
        monitor: Option<std::thread::JoinHandle<()>>,
        pub(crate) breached: mpsc::UnboundedReceiver<LimitBreach>,
    }

    fn watch_limits(
        job: Arc<OwnedHandle>,
        port: Arc<OwnedHandle>,
        stopped: Arc<AtomicBool>,
        send: mpsc::UnboundedSender<LimitBreach>,
    ) {
        while !stopped.load(Ordering::Relaxed) {
            let (mut code, mut key, mut overlapped) = (0u32, 0usize, std::ptr::null_mut());
            let ready = unsafe {
                GetQueuedCompletionStatus(
                    port.as_raw_handle(),
                    &mut code,
                    &mut key,
                    &mut overlapped,
                    100,
                )
            };
            if ready == 0 || key != job.as_raw_handle() as usize {
                continue;
            }
            let reason = match code {
                JOB_OBJECT_MSG_JOB_MEMORY_LIMIT | JOB_OBJECT_MSG_PROCESS_MEMORY_LIMIT => {
                    Some(LimitBreach::Memory)
                }
                JOB_OBJECT_MSG_END_OF_PROCESS_TIME => Some(LimitBreach::Cpu),
                JOB_OBJECT_MSG_ACTIVE_PROCESS_LIMIT => Some(LimitBreach::Processes),
                _ => None,
            };
            if let Some(reason) = reason {
                let _ = send.send(reason);
                unsafe {
                    TerminateJobObject(job.as_raw_handle(), 1);
                }
                break;
            }
        }
    }

    impl Limits {
        pub(crate) fn new() -> Result<Self> {
            Self::with_limits(MEMORY_BYTES, CPU_SECONDS, MAX_PROCESSES)
        }

        pub(crate) fn with_limits(
            memory_bytes: u64,
            cpu_seconds: u64,
            max_processes: u32,
        ) -> Result<Self> {
            let job = Arc::new(handle(unsafe {
                CreateJobObjectW(std::ptr::null(), std::ptr::null())
            })?);
            let mut info = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
            info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE
                | JOB_OBJECT_LIMIT_ACTIVE_PROCESS
                | JOB_OBJECT_LIMIT_JOB_MEMORY
                | JOB_OBJECT_LIMIT_PROCESS_TIME;
            info.BasicLimitInformation.ActiveProcessLimit = max_processes;
            info.BasicLimitInformation.PerProcessUserTimeLimit = (cpu_seconds * 10_000_000) as i64;
            info.JobMemoryLimit = memory_bytes
                .try_into()
                .context("plugin memory limit unsupported on this architecture")?;
            let ok = unsafe {
                SetInformationJobObject(
                    job.as_raw_handle(),
                    JobObjectExtendedLimitInformation,
                    (&info as *const JOBOBJECT_EXTENDED_LIMIT_INFORMATION).cast(),
                    size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
                )
            };
            anyhow::ensure!(
                ok != 0,
                "configure plugin job limits: {}",
                std::io::Error::last_os_error()
            );
            let port = Arc::new(handle(unsafe {
                CreateIoCompletionPort(INVALID_HANDLE_VALUE, std::ptr::null_mut(), 0, 1)
            })?);
            let association = JOBOBJECT_ASSOCIATE_COMPLETION_PORT {
                CompletionKey: job.as_raw_handle(),
                CompletionPort: port.as_raw_handle(),
            };
            let ok = unsafe {
                SetInformationJobObject(
                    job.as_raw_handle(),
                    JobObjectAssociateCompletionPortInformation,
                    (&association as *const JOBOBJECT_ASSOCIATE_COMPLETION_PORT).cast(),
                    size_of::<JOBOBJECT_ASSOCIATE_COMPLETION_PORT>() as u32,
                )
            };
            anyhow::ensure!(
                ok != 0,
                "monitor plugin job limits: {}",
                std::io::Error::last_os_error()
            );
            let (send, breached) = mpsc::unbounded_channel();
            let stopped = Arc::new(AtomicBool::new(false));
            let (watch_job, watch_stop) = (job.clone(), stopped.clone());
            let monitor =
                std::thread::spawn(move || watch_limits(watch_job, port, watch_stop, send));
            Ok(Self {
                job,
                stopped,
                monitor: Some(monitor),
                breached,
            })
        }

        pub(crate) fn attach_and_resume(&self, child: &Child) -> Result<()> {
            let pid = child.id().context("spawned plugin has no process id")?;
            let process =
                handle(unsafe { OpenProcess(PROCESS_SET_QUOTA | PROCESS_TERMINATE, 0, pid) })?;
            let ok = unsafe {
                AssignProcessToJobObject(self.job.as_raw_handle(), process.as_raw_handle())
            };
            anyhow::ensure!(
                ok != 0,
                "assign suspended plugin to job: {}",
                std::io::Error::last_os_error()
            );
            let snapshot = handle(unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0) })?;
            let mut entry = THREADENTRY32 {
                dwSize: size_of::<THREADENTRY32>() as u32,
                ..Default::default()
            };
            let mut found = unsafe { Thread32First(snapshot.as_raw_handle(), &mut entry) };
            while found != 0 {
                if entry.th32OwnerProcessID == pid {
                    let thread = handle(unsafe {
                        OpenThread(THREAD_SUSPEND_RESUME, 0, entry.th32ThreadID)
                    })?;
                    anyhow::ensure!(
                        unsafe { ResumeThread(thread.as_raw_handle()) } != u32::MAX,
                        "resume confined plugin: {}",
                        std::io::Error::last_os_error()
                    );
                    return Ok(());
                }
                found = unsafe { Thread32Next(snapshot.as_raw_handle(), &mut entry) };
            }
            anyhow::bail!("suspended plugin thread unavailable")
        }

        pub(crate) fn terminate(&self) {
            unsafe {
                TerminateJobObject(self.job.as_raw_handle(), 1);
            }
        }
    }

    impl Drop for Limits {
        fn drop(&mut self) {
            self.stopped.store(true, Ordering::Relaxed);
            self.terminate();
            if let Some(monitor) = self.monitor.take() {
                let _ = monitor.join();
            }
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use std::{process::Stdio, time::Duration};
        use tokio::process::Command;
        use windows_sys::Win32::System::Threading::CREATE_SUSPENDED;

        #[test]
        #[ignore]
        fn sidecar_resource_child() {
            match std::env::var("POSTAL_TEST_RESOURCE").as_deref() {
                Ok("memory") => {
                    let mut blocks = Vec::new();
                    loop {
                        blocks.push(vec![0x5a_u8; 4 * 1024 * 1024]);
                    }
                }
                Ok("cpu") => loop {
                    std::hint::black_box(1u64.wrapping_mul(7));
                },
                Ok("processes") => {
                    let _ = std::process::Command::new(std::env::current_exe().unwrap())
                        .arg("--help")
                        .status();
                    std::thread::sleep(Duration::from_secs(5));
                }
                _ => panic!("resource child mode missing"),
            }
        }

        async fn breach(mode: &str, expected: LimitBreach, max_processes: u32) {
            let mut command = Command::new(std::env::current_exe().unwrap());
            command
                .args(["sidecar_resource_child", "--ignored", "--nocapture"])
                .env("POSTAL_TEST_RESOURCE", mode)
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .creation_flags(CREATE_SUSPENDED | 0x08000000)
                .kill_on_drop(true);
            let mut limits = Limits::with_limits(64 * 1024 * 1024, 1, max_processes).unwrap();
            let mut child = command.spawn().unwrap();
            limits.attach_and_resume(&child).unwrap();
            let observed = tokio::time::timeout(Duration::from_secs(15), limits.breached.recv())
                .await
                .unwrap()
                .unwrap();
            assert_eq!(observed, expected);
            let _ = tokio::time::timeout(Duration::from_secs(3), child.wait()).await;
        }

        #[tokio::test]
        async fn memory_limit_stops_only_owned_job() {
            breach("memory", LimitBreach::Memory, 8).await;
        }

        #[tokio::test]
        async fn cpu_limit_stops_only_owned_job() {
            breach("cpu", LimitBreach::Cpu, 8).await;
        }

        #[tokio::test]
        async fn process_limit_stops_only_owned_job() {
            breach("processes", LimitBreach::Processes, 1).await;
        }
    }
}

#[cfg(unix)]
mod platform {
    use super::*;
    use std::os::unix::process::CommandExt;
    use tokio::process::Command;

    pub(crate) struct Limits {
        pid: i32,
        armed: bool,
    }

    impl Limits {
        pub(crate) fn configure(command: &mut Command) {
            unsafe {
                command.as_std_mut().pre_exec(|| {
                    if libc::setsid() < 0 {
                        return Err(std::io::Error::last_os_error());
                    }
                    let address = libc::rlimit {
                        rlim_cur: MEMORY_BYTES as libc::rlim_t,
                        rlim_max: MEMORY_BYTES as libc::rlim_t,
                    };
                    let cpu = libc::rlimit {
                        rlim_cur: CPU_SECONDS as libc::rlim_t,
                        rlim_max: (CPU_SECONDS + 2) as libc::rlim_t,
                    };
                    if libc::setrlimit(libc::RLIMIT_AS, &address) < 0
                        || libc::setrlimit(libc::RLIMIT_CPU, &cpu) < 0
                    {
                        return Err(std::io::Error::last_os_error());
                    }
                    Ok(())
                });
            }
        }
        pub(crate) fn new(pid: u32) -> Self {
            Self {
                pid: pid as i32,
                armed: true,
            }
        }
        pub(crate) fn terminate(&mut self) {
            if self.armed {
                unsafe {
                    libc::kill(-self.pid, libc::SIGKILL);
                }
                self.armed = false;
            }
        }
        pub(crate) fn disarm(&mut self) {
            self.armed = false;
        }
        pub(crate) fn exited_unreaped(&self) -> std::io::Result<bool> {
            loop {
                let mut info: libc::siginfo_t = unsafe { std::mem::zeroed() };
                let result = unsafe {
                    libc::waitid(
                        libc::P_PID,
                        self.pid as libc::id_t,
                        &mut info,
                        libc::WEXITED | libc::WNOHANG | libc::WNOWAIT,
                    )
                };
                if result == 0 {
                    return Ok(unsafe { info.si_pid() } != 0);
                }
                let error = std::io::Error::last_os_error();
                if error.raw_os_error() != Some(libc::EINTR) {
                    return Err(error);
                }
            }
        }
    }

    pub(crate) fn exit_limit(status: std::process::ExitStatus) -> Option<LimitBreach> {
        use std::os::unix::process::ExitStatusExt;
        (status.signal() == Some(libc::SIGXCPU)).then_some(LimitBreach::Cpu)
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use std::{process::Stdio, time::Duration};
        use tokio::process::Command;

        #[cfg(target_os = "macos")]
        fn vm_size(pid: i32) -> Option<u64> {
            let mut info: libc::proc_taskinfo = unsafe { std::mem::zeroed() };
            let size = std::mem::size_of::<libc::proc_taskinfo>() as i32;
            let read = unsafe { libc::proc_pidinfo(pid, libc::PROC_PIDTASKINFO, 0, (&mut info as *mut libc::proc_taskinfo).cast(), size) };
            (read == size).then_some(info.pti_virtual_size)
        }

        #[cfg(target_os = "macos")]
        fn probe_limit_stage(stage: u8) -> Result<Option<u64>, String> {
            use std::os::unix::process::CommandExt as _;
            let mut command = std::process::Command::new("/bin/sleep");
            command.arg("30").stdout(Stdio::null()).stderr(Stdio::null());
            if stage != 0 {
                unsafe {
                    command.pre_exec(move || {
                        let address = libc::rlimit { rlim_cur: MEMORY_BYTES as libc::rlim_t, rlim_max: MEMORY_BYTES as libc::rlim_t };
                        let cpu = libc::rlimit { rlim_cur: CPU_SECONDS as libc::rlim_t, rlim_max: (CPU_SECONDS + 2) as libc::rlim_t };
                        let result = match stage {
                            1 => libc::setsid(),
                            2 => libc::setrlimit(libc::RLIMIT_AS, &address),
                            3 => libc::setrlimit(libc::RLIMIT_CPU, &cpu),
                            _ => 0,
                        };
                        if result < 0 { Err(std::io::Error::last_os_error()) } else { Ok(()) }
                    });
                }
            }
            let mut child = command.spawn().map_err(|error| error.to_string())?;
            let vm = vm_size(child.id() as i32);
            let killed = child.kill();
            let reaped = child.wait();
            killed.map_err(|error| format!("kill probe child: {error}"))?;
            reaped.map_err(|error| format!("reap probe child: {error}"))?;
            Ok(vm)
        }

        #[cfg(target_os = "macos")]
        #[test]
        fn darwin_spawn_limit_stage_diagnostic() {
            let mut inherited: libc::rlimit = unsafe { std::mem::zeroed() };
            let rlimit = if unsafe { libc::getrlimit(libc::RLIMIT_AS, &mut inherited) } == 0 {
                Some((inherited.rlim_cur, inherited.rlim_max))
            } else { None };
            let parent_vm = vm_size(unsafe { libc::getpid() });
            let plain = probe_limit_stage(0);
            let session = probe_limit_stage(1);
            let address = probe_limit_stage(2);
            let cpu = probe_limit_stage(3);
            assert!(parent_vm.is_some() && matches!(&plain, Ok(Some(_)))
                && session.is_ok() && address.is_ok() && cpu.is_ok(),
                "parent_vm={parent_vm:?} fresh_exec_vm={plain:?} inherited_rlimit_as={rlimit:?} setsid={session:?} rlimit_as={address:?} rlimit_cpu={cpu:?}");
        }

        #[test]
        #[ignore]
        fn owned_group_child() {
            std::thread::sleep(Duration::from_secs(30));
        }

        #[tokio::test]
        async fn group_is_disarmed_before_reaping_leader() {
            let mut command = Command::new(std::env::current_exe().unwrap());
            command
                .args(["owned_group_child", "--ignored", "--nocapture"])
                .stdout(Stdio::null())
                .stderr(Stdio::null());
            Limits::configure(&mut command);
            let mut child = command.spawn().unwrap();
            let mut limits = Limits::new(child.id().unwrap());
            assert!(!limits.exited_unreaped().unwrap());
            limits.terminate();
            assert!(!limits.armed);
            assert!(!tokio::time::timeout(Duration::from_secs(3), child.wait())
                .await
                .unwrap()
                .unwrap()
                .success());
            limits.terminate();
        }
    }
}

pub(crate) use platform::*;
