//! Taking a lock whose last holder panicked. Most commands run on Tauri's own threads, which catch a panic: the command
//! fails and the program goes on (`panic = "unwind"`). A std lock that command held is left poisoned, and every later
//! `.lock().unwrap()` on it would panic too, so nothing that needs it works again. These take the lock anyway. Only for
//! data a panic can't leave half-changed (shared/README.md, "A panic and the locks").

use std::sync::{Mutex, MutexGuard, PoisonError, RwLock, RwLockReadGuard, RwLockWriteGuard};

pub fn lock<T: ?Sized>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

pub fn read_lock<T: ?Sized>(l: &RwLock<T>) -> RwLockReadGuard<'_, T> {
    l.read().unwrap_or_else(PoisonError::into_inner)
}

pub fn write_lock<T: ?Sized>(l: &RwLock<T>) -> RwLockWriteGuard<'_, T> {
    l.write().unwrap_or_else(PoisonError::into_inner)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    #[test]
    fn a_panic_while_holding_it_leaves_the_data_reachable() {
        let m = Arc::new(Mutex::new(vec![1]));
        let r = Arc::new(RwLock::new(String::from("kept")));
        let (m2, r2) = (m.clone(), r.clone());
        let died = std::thread::spawn(move || {
            let mut v = m2.lock().unwrap();
            let _w = r2.write().unwrap();
            v.push(2);
            panic!("a command trips while holding both");
        })
        .join();
        assert!(died.is_err() && m.is_poisoned() && r.is_poisoned());
        assert_eq!(*lock(&m), vec![1, 2]);
        assert_eq!(*read_lock(&r), "kept");
        write_lock(&r).push('!');
        assert_eq!(*read_lock(&r), "kept!");
    }
}
