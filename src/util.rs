use std::sync::{
    mpsc::{Receiver, Sender, SyncSender},
    Arc, Mutex, MutexGuard,
};

/// The idea of lock free is that the type is free of primitives that could lock
/// across threads. This means e.g. the closure captures neither mutexes nor
/// channels. But there is nothing wrong with spawning channels or mutexes in
/// such a scope because they can never leave as the
/// # Safety
/// This should only be implemented for a type if you can guarantee that
/// releasing it will not possibly cause deadlocks. This may be possible for a
/// struct wrapping channels that gurantee the order of flow, and don't
/// communicate with any other channels. I can't immediately see how one could
/// guarantee lock avoidance, but in the spirit of unsafe, I shouldn't stop you
/// from "Hold my beer and watch this". You just gotta say it.
pub unsafe auto trait LockFree {}

/// The idea here is to introduce a struct that forbids waiting, so we can
/// guarantee not holding a mutex across an awai.
/// # Safety
/// don't use
// pub unsafe auto trait AwaitFree {}
pub trait AwaitFree {}

impl<T> !LockFree for Mutex<T> {}
impl<'a, T> !LockFree for MutexGuard<'a, T> {}

impl<T> !LockFree for Sender<T> {}
impl<T> !LockFree for SyncSender<T> {}
impl<T> !LockFree for Receiver<T> {}

unsafe impl LockFree for reqwest::Client {}
// unsafe impl LockFree for tokio::park::Unpark {}
// unsafe impl LockFree for std::sync::Mutex<hyper::client::pool::PoolInner<hyper::client::client::PoolClient<reqwest::async_impl::body::ImplStream>>> {}
// unsafe impl LockFree for Mutex<tokio::time::driver::InnerState> {}

pub(crate) trait SafeComputation<I: SafeType, O: SafeType> =
    Fn(I) -> O + Send + Sync + LockFree + 'static;

pub(crate) trait SafeRefComputation<I: SafeType, O: SafeType> =
    Fn(I) -> O + Send + Sync + LockFree + 'static;

pub(crate) trait SafeMutRefComputation<I: SafeType, O: SafeType> =
    Fn(&mut I) -> O + Send + Sync + LockFree + 'static;

//todo: add AwaitFree
pub trait SafeType = Send + Sync + LockFree + Sized + Clone + 'static;

pub(crate) type SharedMutex<T> = Arc<Mutex<T>>;

pub fn new_shared<T>(t: T) -> SharedMutex<T> {
    Arc::new(Mutex::new(t))
}
