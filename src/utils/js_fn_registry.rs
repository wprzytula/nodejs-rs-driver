//! Per-environment registries of JS functions that Rust calls back into.
//!
//! JS hands Rust a function once, at module load time (a class constructor, or a callback used to
//! build up a JS collection); Rust looks it up later, whenever it has data to give back. Both
//! `define_js_ctor!` and `define_js_callback!` in [`crate::utils::js_ctor`] are thin wrappers over
//! [`JsFnRegistry`].
//!
//! A single OS thread can host multiple, independent N-API environments (one per `worker_threads`
//! worker, plus the main thread), so every registry is keyed by `napi_env`.

use std::collections::HashMap;
use std::sync::Mutex;

use napi::Env;
use napi::bindgen_prelude::{
    FromNapiValue, Function, FunctionRef, JsValue, JsValuesTupleIntoVec, Unknown,
};

/// The registration slot for one JS function, holding one entry per N-API environment.
///
/// The registry has to be a single `static`, so its value type is fixed at compile time - but the
/// argument types of the functions stored in it are often borrowed, with a lifetime belonging to
/// one specific call rather than to the `'static` registry. Every `FunctionRef` is therefore stored
/// under the erased `Function<(), ()>` type, and re-tagged to the call's real argument type at the
/// point of use (see [`JsCallback::call`] and `define_js_ctor!`'s `build_*`).
pub(crate) struct JsFnRegistry {
    /// What is being registered, e.g. `"Host constructor"`. Used only in error messages.
    what: &'static str,
    entries: Mutex<Option<HashMap<usize, FunctionRef<(), ()>>>>,
}

impl JsFnRegistry {
    pub(crate) const fn new(what: &'static str) -> Self {
        JsFnRegistry {
            what,
            entries: Mutex::new(None),
        }
    }

    /// Stores `func` as this environment's entry. Called exactly once per environment, from JS.
    pub(crate) fn register(&'static self, func: Function<(), ()>, env: Env) -> napi::Result<()> {
        let func_ref = func.create_ref()?;
        let key = env.raw() as usize;
        {
            let entries = self.entries.lock().unwrap();
            if entries
                .as_ref()
                .is_some_and(|entries| entries.contains_key(&key))
            {
                return Err(napi::Error::from_reason(format!(
                    "{} is already registered in this environment; \
                     registration must happen exactly once per environment",
                    self.what
                )));
            }
        }
        // Remove this environment's entry, dropping its `FunctionRef`, exactly when this specific
        // environment is torn down - rather than leaving it to linger past its environment's
        // lifetime, or be silently reused by a later environment that happens to be allocated at
        // the same address.
        env.add_env_cleanup_hook(key, move |key| {
            if let Some(entries) = self.entries.lock().unwrap().as_mut() {
                entries.remove(&key);
            }
        })?;
        self.entries
            .lock()
            .unwrap()
            .get_or_insert_with(HashMap::new)
            .insert(key, func_ref);
        Ok(())
    }

    /// Borrows this environment's entry back into the current handle scope.
    pub(crate) fn borrow<'env>(
        &'static self,
        env: &'env Env,
    ) -> napi::Result<Function<'env, (), ()>> {
        let key = env.raw() as usize;
        let entries = self.entries.lock().unwrap();
        let func_ref = entries
            .as_ref()
            .and_then(|entries| entries.get(&key))
            .ok_or_else(|| {
                napi::Error::from_reason(format!(
                    "{} is not registered yet; ensure the correct module has been loaded \
                     before accessing cluster metadata",
                    self.what
                ))
            })?;
        func_ref.borrow_back(env)
    }
}

/// A registered JS function, borrowed out of its [`JsFnRegistry`] once and then invoked many times.
///
/// Borrowing once and calling repeatedly is the point: it takes the registry lock and the
/// `napi_get_reference_value` hop out of the per-item path, which matters when the callback is
/// invoked once per node, column or keyspace.
///
/// The argument type is erased here and re-tagged per call; `define_js_callback!` wraps this in a
/// per-callback handle type that pins the argument type down again.
pub(crate) struct JsCallback<'env> {
    func: Function<'env, (), ()>,
}

impl<'env> JsCallback<'env> {
    pub(crate) fn new(func: Function<'env, (), ()>) -> Self {
        JsCallback { func }
    }

    /// Invokes the callback, discarding its return value.
    ///
    /// A JS exception thrown by the callback surfaces here as `Err`, so callers iterating over a
    /// sequence stop at the first failure.
    pub(crate) fn call<A: JsValuesTupleIntoVec>(&self, args: A) -> napi::Result<()> {
        self.retagged::<A, Unknown>()?.call(args).map(|_| ())
    }

    /// Invokes the callback and converts its return value to `R`.
    #[expect(
        dead_code,
        reason = "part of the JsCallback API, not yet needed by any caller"
    )]
    pub(crate) fn call_returning<A: JsValuesTupleIntoVec, R: FromNapiValue>(
        &self,
        args: A,
    ) -> napi::Result<R> {
        self.retagged::<A, R>()?.call(args)
    }

    /// Re-tags the erased handle's phantom `Args` type parameter to this call's real (possibly
    /// borrowed) argument type. This is a type-level operation only: the underlying `napi_value`
    /// is unchanged.
    fn retagged<A: JsValuesTupleIntoVec, R>(&self) -> napi::Result<Function<'_, A, R>> {
        unsafe { Function::from_napi_value(self.func.value().env, self.func.raw()) }
    }
}
