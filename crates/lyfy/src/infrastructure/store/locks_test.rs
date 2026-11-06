use crate::infrastructure::store::Lock;

#[test]
fn a_forked_child_cannot_extend_a_completed_operations_lock() {
    let temp = tempfile::tempdir().unwrap();
    let directory = temp.path().join("locks");
    let lock = Lock::acquire(&directory).unwrap();
    let mut pipe = [-1; 2];
    // SAFETY: pipe has space for both descriptors; fork's child uses only libc
    // calls and exits without touching Rust runtime state or running destructors.
    let child = unsafe {
        assert_eq!(libc::pipe2(pipe.as_mut_ptr(), libc::O_CLOEXEC), 0);
        let child = libc::fork();
        if child == 0 {
            libc::close(pipe[1]);
            let mut byte = 0u8;
            libc::read(pipe[0], (&mut byte as *mut u8).cast(), 1);
            libc::_exit(0);
        }
        child
    };
    assert!(child > 0);
    drop(lock);
    let next = Lock::acquire(&directory);
    // SAFETY: valid pipe ends and child PID; release and reap the child before
    // asserting so a regression never leaves a waiting process behind.
    unsafe {
        libc::close(pipe[0]);
        let byte = 0u8;
        assert_eq!(libc::write(pipe[1], (&byte as *const u8).cast(), 1), 1);
        libc::close(pipe[1]);
        assert_eq!(libc::waitpid(child, std::ptr::null_mut(), 0), child);
    }
    assert!(
        next.is_ok(),
        "child retained the completed lock: {:?}",
        next.err()
    );
}
