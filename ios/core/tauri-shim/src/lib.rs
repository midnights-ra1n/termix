//! Stands in for the few Tauri items `src-tauri/src/syncd.rs` uses (AppHandle, state, emit, async runtime),
//! so the iOS core compiles that file unchanged instead of keeping a copy of the sync loop.
use std::any::{Any, TypeId};
use std::sync::{OnceLock, RwLock};

pub use tauri_shim_macros::command;

#[derive(Clone, Default)]
pub struct AppHandle;

/// A managed value, like Tauri's `State`.
pub struct State<T: 'static>(&'static T);

impl<T> State<T> {
    pub fn inner(&self) -> &'static T {
        self.0
    }
}

impl<T> std::ops::Deref for State<T> {
    type Target = T;
    fn deref(&self) -> &T {
        self.0
    }
}

type Managed = (TypeId, &'static (dyn Any + Send + Sync));
static STATES: RwLock<Vec<Managed>> = RwLock::new(Vec::new());
static EMIT: OnceLock<Box<dyn Fn(&str, String) + Send + Sync>> = OnceLock::new();

impl AppHandle {
    /// Like Tauri's `manage`: one value per type, alive for the whole process.
    pub fn manage<T: Send + Sync + 'static>(&self, v: T) {
        STATES.write().unwrap().push((TypeId::of::<T>(), Box::leak(Box::new(v))));
    }
    /// Where `emit` sends events: (name, JSON payload).
    pub fn on_emit(&self, f: impl Fn(&str, String) + Send + Sync + 'static) {
        let _ = EMIT.set(Box::new(f));
    }
}

pub trait Manager {
    fn state<T: Send + Sync + 'static>(&self) -> State<T>;
}

impl Manager for AppHandle {
    fn state<T: Send + Sync + 'static>(&self) -> State<T> {
        let all = STATES.read().unwrap();
        let (_, v) = all.iter().find(|(id, _)| *id == TypeId::of::<T>()).expect("state not managed");
        State(v.downcast_ref().unwrap())
    }
}

pub trait Emitter {
    fn emit<S: serde::Serialize>(&self, event: &str, payload: S) -> Result<(), String>;
}

impl Emitter for AppHandle {
    fn emit<S: serde::Serialize>(&self, event: &str, payload: S) -> Result<(), String> {
        let json = serde_json::to_string(&payload).map_err(|e| e.to_string())?;
        if let Some(f) = EMIT.get() {
            f(event, json);
        }
        Ok(())
    }
}

pub mod async_runtime {
    use std::future::Future;
    use std::sync::OnceLock;
    use tokio::runtime::Runtime;
    use tokio::task::JoinHandle;

    pub fn runtime() -> &'static Runtime {
        static RT: OnceLock<Runtime> = OnceLock::new();
        RT.get_or_init(|| tokio::runtime::Builder::new_multi_thread().enable_all().build().expect("tokio runtime"))
    }

    pub fn spawn<F: Future + Send + 'static>(f: F) -> JoinHandle<F::Output>
    where
        F::Output: Send + 'static,
    {
        runtime().spawn(f)
    }
}
