//! The handle registry: native objects are owned here and Java holds opaque `long` ids,
//! never pointers.
//!
//! Ids come from one process-wide counter shared by every kind of object, start at 1 and
//! are never reused, so a stale, forged or wrong-kind handle is detected (it is simply not
//! in the table) instead of aliasing another object. Objects are reference counted: a call
//! takes a clone of the object's `Arc` and works on it outside the table lock, so closing
//! a handle while another thread still uses the object is safe (the object is dropped
//! when the last user finishes).

use crate::error::{Error, HandleKind, Result};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};

/// The next id handed out (shared by all registries).
static NEXT_ID: AtomicU64 = AtomicU64::new(1);

/// A table of live objects of one kind, keyed by handle.
pub(crate) struct Registry<T> {
    kind: HandleKind,
    objects: Mutex<HashMap<u64, Arc<Mutex<T>>>>,
}

impl<T> Registry<T> {
    /// An empty registry for objects of `kind`.
    pub(crate) fn new(kind: HandleKind) -> Self {
        Self { kind, objects: Mutex::new(HashMap::new()) }
    }

    fn table(&self) -> MutexGuard<'_, HashMap<u64, Arc<Mutex<T>>>> {
        // The table is only modified by single `insert`/`remove` calls, so it is consistent
        // even if a panic poisoned the lock.
        self.objects.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Store `value` and return its new handle (never 0, and far below `i64::MAX`, so it
    /// is a positive Java `long`).
    pub(crate) fn insert(&self, value: T) -> u64 {
        let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
        self.table().insert(id, Arc::new(Mutex::new(value)));
        id
    }

    /// The object behind `handle`.
    pub(crate) fn get(&self, handle: u64) -> Result<Arc<Mutex<T>>> {
        self.table().get(&handle).cloned().ok_or(Error::UnknownHandle { kind: self.kind, handle })
    }

    /// Forget `handle`; the object is dropped once no call uses it any more.
    pub(crate) fn remove(&self, handle: u64) -> Result<()> {
        self.table().remove(&handle).map(drop).ok_or(Error::UnknownHandle { kind: self.kind, handle })
    }

    /// Number of live handles.
    #[cfg(test)]
    pub(crate) fn len(&self) -> usize {
        self.table().len()
    }
}

/// Objects that can repair themselves after a panic poisoned their lock.
pub(crate) trait Recover {
    /// Drop every piece of state a panic may have left half-updated.
    fn recover(&mut self);
}

/// Lock an object. If a previous call panicked while holding the lock, the object is told
/// to [`Recover`] and the poison is cleared.
pub(crate) fn lock<T: Recover>(object: &Mutex<T>) -> MutexGuard<'_, T> {
    match object.lock() {
        Ok(guard) => guard,
        Err(poisoned) => {
            let mut guard = poisoned.into_inner();
            guard.recover();
            object.clear_poison();
            guard
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Counter(u32);
    impl Recover for Counter {
        fn recover(&mut self) {
            self.0 = 0;
        }
    }

    #[test]
    fn handles_are_unique_positive_and_kind_checked() {
        let a: Registry<Counter> = Registry::new(HandleKind::Session);
        let b: Registry<Counter> = Registry::new(HandleKind::Evaluator);
        let h1 = a.insert(Counter(1));
        let h2 = b.insert(Counter(2));
        assert!(h1 > 0 && h2 > h1);
        assert!(a.get(h1).is_ok());
        // A handle of another kind is unknown here.
        let e = a.get(h2).err().unwrap();
        assert_eq!(e.to_string(), format!("unknown or closed pack session handle {h2}"));
        for bad in [0, u64::MAX, h2 + 1000] {
            assert!(a.get(bad).is_err());
        }
        // Removing keeps users alive; the handle is gone and never reused.
        let alive = a.get(h1).unwrap();
        a.remove(h1).unwrap();
        assert!(a.remove(h1).is_err());
        assert!(a.get(h1).is_err());
        assert_eq!(alive.lock().unwrap().0, 1);
        assert!(a.insert(Counter(3)) > h2);
        assert_eq!(a.len(), 1);
    }

    #[test]
    fn poisoned_objects_recover() {
        let r: Registry<Counter> = Registry::new(HandleKind::Session);
        let h = r.insert(Counter(5));
        let obj = r.get(h).unwrap();
        let o2 = obj.clone();
        let _ = std::thread::spawn(move || {
            let _g = o2.lock().unwrap();
            panic!("poison");
        })
        .join();
        assert!(obj.is_poisoned());
        assert_eq!(lock(&obj).0, 0);
        assert!(!obj.is_poisoned());
    }
}
