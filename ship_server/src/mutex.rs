use parking_lot::{
    lock_api::{RawMutex as _, RawRwLock},
    Mutex as PMutex, MutexGuard as PGuard, RwLock as PRwLock, RwLockReadGuard as PRwReadGuard,
    RwLockWriteGuard as PRwWriteGuard,
};
use std::{
    future::Future,
    ops::{Deref, DerefMut},
};

// let's reinvent the wheel
pub struct Mutex<T> {
    mutex: PMutex<T>,
}
pub struct MutexGuard<'a, T> {
    guard: PGuard<'a, T>,
}
pub struct RwLock<T> {
    lock: PRwLock<T>,
}
pub struct RwReadGuard<'a, T> {
    guard: PRwReadGuard<'a, T>,
}
pub struct RwWriteGuard<'a, T> {
    guard: PRwWriteGuard<'a, T>,
}

impl<T> Mutex<T> {
    pub const fn new(val: T) -> Mutex<T> {
        Self {
            mutex: PMutex::new(val),
        }
    }
    pub async fn lock(&self) -> MutexGuard<T>
    where
        Self: Send,
        T: Send,
    {
        loop {
            match self.mutex.try_lock() {
                Some(guard) => return MutexGuard { guard },
                None => tokio::task::yield_now().await,
            }
        }
    }
    pub fn lock_blocking(&self) -> MutexGuard<T> {
        MutexGuard {
            guard: self.mutex.lock(),
        }
    }
}

impl<T> Deref for MutexGuard<'_, T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        &self.guard
    }
}
impl<T> DerefMut for MutexGuard<'_, T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.guard
    }
}

impl<'a, T> MutexGuard<'a, T> {
    pub async fn unlocked<F, R>(s: &mut Self, f: F) -> R
    where
        Self: Send,
        F: FnOnce() -> R + Send,
        R: Send,
    {
        let mutex = PGuard::mutex(&s.guard);
        //SAFETY: same as below
        let raw = unsafe { mutex.raw() };
        //SAFETY: mut ref guarantees that there is only one ref to the underlying data
        unsafe { raw.unlock() };
        let out = f();
        loop {
            match raw.try_lock() {
                true => return out,
                false => tokio::task::yield_now().await,
            }
        }
    }
    pub async fn unlocked_async<F, Fu>(s: &mut Self, f: F) -> Fu::Output
    where
        Self: Send,
        F: FnOnce() -> Fu + Send,
        Fu: Future + Send,
        Fu::Output: Send,
    {
        let mutex = PGuard::mutex(&s.guard);
        //SAFETY: same as below
        let raw = unsafe { mutex.raw() };
        //SAFETY: mut ref guarantees that there is only one ref to the underlying data
        unsafe { raw.unlock() };
        let out = f().await;
        loop {
            match raw.try_lock() {
                true => return out,
                false => tokio::task::yield_now().await,
            }
        }
    }
}

impl<T> RwLock<T> {
    pub const fn new(val: T) -> RwLock<T> {
        Self {
            lock: PRwLock::new(val),
        }
    }
    pub async fn read(&self) -> RwReadGuard<T>
    where
        Self: Send,
        T: Send + Sync,
    {
        loop {
            match self.lock.try_read() {
                Some(guard) => return RwReadGuard { guard },
                None => tokio::task::yield_now().await,
            }
        }
    }
    pub fn read_blocking(&self) -> RwReadGuard<T> {
        RwReadGuard {
            guard: self.lock.read(),
        }
    }
    pub async fn write(&self) -> RwWriteGuard<T>
    where
        Self: Send,
        T: Send + Sync,
    {
        loop {
            match self.lock.try_write() {
                Some(guard) => return RwWriteGuard { guard },
                None => tokio::task::yield_now().await,
            }
        }
    }
    pub fn write_blocking(&self) -> RwWriteGuard<T> {
        RwWriteGuard {
            guard: self.lock.write(),
        }
    }
}

impl<T> Deref for RwReadGuard<'_, T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        &self.guard
    }
}

impl<T> Deref for RwWriteGuard<'_, T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        &self.guard
    }
}
impl<T> DerefMut for RwWriteGuard<'_, T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.guard
    }
}

impl<'a, T> RwReadGuard<'a, T> {
    pub async fn unlocked<F, R>(s: &mut Self, f: F) -> R
    where
        Self: Send,
        F: FnOnce() -> R + Send,
        R: Send,
    {
        let rwlock = PRwReadGuard::rwlock(&s.guard);
        //SAFETY: same as below
        let raw = unsafe { rwlock.raw() };
        //SAFETY: mut ref guarantees that there is only one ref to the underlying data
        unsafe { raw.unlock_shared() };
        let out = f();
        loop {
            match raw.try_lock_shared() {
                true => return out,
                false => tokio::task::yield_now().await,
            }
        }
    }
    pub async fn unlocked_async<F, Fu>(s: &mut Self, f: F) -> Fu::Output
    where
        Self: Send,
        F: FnOnce() -> Fu + Send,
        Fu: Future + Send,
        Fu::Output: Send,
    {
        let rwlock = PRwReadGuard::rwlock(&s.guard);
        //SAFETY: same as below
        let raw = unsafe { rwlock.raw() };
        //SAFETY: mut ref guarantees that there is only one ref to the underlying data
        unsafe { raw.unlock_shared() };
        let out = f().await;
        loop {
            match raw.try_lock_shared() {
                true => return out,
                false => tokio::task::yield_now().await,
            }
        }
    }
}

impl<'a, T> RwWriteGuard<'a, T> {
    pub async fn unlocked<F, R>(s: &mut Self, f: F) -> R
    where
        Self: Send,
        F: FnOnce() -> R + Send,
        R: Send,
    {
        let rwlock = PRwWriteGuard::rwlock(&s.guard);
        //SAFETY: same as below
        let raw = unsafe { rwlock.raw() };
        //SAFETY: mut ref guarantees that there is only one ref to the underlying data
        unsafe { raw.unlock_exclusive() };
        let out = f();
        loop {
            match raw.try_lock_exclusive() {
                true => return out,
                false => tokio::task::yield_now().await,
            }
        }
    }
    pub async fn unlocked_async<F, Fu>(s: &mut Self, f: F) -> Fu::Output
    where
        Self: Send,
        F: FnOnce() -> Fu + Send,
        Fu: Future + Send,
        Fu::Output: Send,
    {
        let rwlock = PRwWriteGuard::rwlock(&s.guard);
        //SAFETY: same as below
        let raw = unsafe { rwlock.raw() };
        //SAFETY: mut ref guarantees that there is only one ref to the underlying data
        unsafe { raw.unlock_exclusive() };
        let out = f().await;
        loop {
            match raw.try_lock_exclusive() {
                true => return out,
                false => tokio::task::yield_now().await,
            }
        }
    }
}
