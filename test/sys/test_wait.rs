use nix::unistd::*;
use nix::unistd::ForkResult::*;
use nix::sys::signal::*;
use nix::sys::wait::*;
use libc::_exit;

#[test]
fn test_wait_signal() {
    #[allow(unused_variables)]
    let m = ::FORK_MTX.lock().expect("Mutex got poisoned by another test");

    // Safe: The child only calls `pause` and/or `_exit`, which are async-signal-safe.
    match fork() {
      Ok(Child) => pause().unwrap_or_else(|_| unsafe { _exit(123) }),
      Ok(Parent { child }) => {
          kill(child, Some(SIGKILL)).ok().expect("Error: Kill Failed");
          assert_eq!(waitpid(child, None), Ok(WaitStatus::Signaled(child, SIGKILL, false)));
      },
      // panic, fork should never fail unless there is a serious problem with the OS
      Err(_) => panic!("Error: Fork Failed")
    }
}

#[test]
fn test_wait_exit() {
    #[allow(unused_variables)]
    let m = ::FORK_MTX.lock().expect("Mutex got poisoned by another test");

    // Safe: Child only calls `_exit`, which is async-signal-safe.
    match fork() {
      Ok(Child) => unsafe { _exit(12); },
      Ok(Parent { child }) => {
          assert_eq!(waitpid(child, None), Ok(WaitStatus::Exited(child, 12)));
      },
      // panic, fork should never fail unless there is a serious problem with the OS
      Err(_) => panic!("Error: Fork Failed")
    }
}

#[test]
fn test_waitstatus_pid() {
    let _m = ::FORK_MTX.lock().expect("Mutex got poisoned by another test");

    match fork().unwrap() {
        Child => unsafe { _exit(0) },
        Parent { child } => {
            let status = waitpid(child, None).unwrap();
            assert_eq!(status.pid(), Some(child));
        }
    }
}

#[cfg(any(target_os = "linux", target_os = "android"))]
// FIXME: qemu-user doesn't implement ptrace on most arches
#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
mod ptrace {
    use nix::sys::ptrace::{self, Options, Event};
    use nix::sys::signal::*;
    use nix::sys::wait::*;
    use nix::unistd::*;
    use nix::unistd::ForkResult::*;
    use libc::_exit;

    fn ptrace_child() -> ! {
        ptrace::traceme().unwrap();
        // As recommended by ptrace(2), raise SIGTRAP to pause the child
        // until the parent is ready to continue
        raise(SIGTRAP).unwrap();
        unsafe { _exit(0) }
    }

    fn ptrace_parent(child: Pid) {
        // Wait for the raised SIGTRAP
        assert_eq!(waitpid(child, None), Ok(WaitStatus::Stopped(child, SIGTRAP)));
        // We want to test a syscall stop and a PTRACE_EVENT stop
        assert!(ptrace::setoptions(child, Options::PTRACE_O_TRACESYSGOOD | Options::PTRACE_O_TRACEEXIT).is_ok());

        // First, stop on the next system call, which will be exit()
        assert!(ptrace::syscall(child).is_ok());
        assert_eq!(waitpid(child, None), Ok(WaitStatus::PtraceSyscall(child)));
        // Then get the ptrace event for the process exiting
        assert!(ptrace::cont(child, None).is_ok());
        assert_eq!(waitpid(child, None), Ok(WaitStatus::PtraceEvent(child, SIGTRAP, Event::PTRACE_EVENT_EXIT as i32)));
        // Finally get the normal wait() result, now that the process has exited
        assert!(ptrace::cont(child, None).is_ok());
        assert_eq!(waitpid(child, None), Ok(WaitStatus::Exited(child, 0)));
    }

    #[test]
    fn test_wait_ptrace() {
        #[allow(unused_variables)]
        let m = ::FORK_MTX.lock().expect("Mutex got poisoned by another test");

        match fork() {
            Ok(Child) => ptrace_child(),
            Ok(Parent { child }) => ptrace_parent(child),
            Err(_) => panic!("Error: Fork Failed")
        }
    }
}
