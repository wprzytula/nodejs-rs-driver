use std::marker::PhantomData;

use napi::bindgen_prelude::{JsValue, Object, ToNapiValue};
use napi::sys;

/// A JavaScript `Object` handle statically tagged with the JS class `C` it is an instance of.
///
/// This is a thin, zero-overhead wrapper over `Object<'env>`: the `C` type parameter carries no
/// runtime data and exists purely so that the Rust type system can tell apart from each other
/// two different `JsInstance` types, even though at the N-API level both are just untyped `Object`s.
/// It makes handing the wrong kind of JS object to a function that expects a specific class
/// a compile error.
///
/// The `'env` lifetime is inherited from the underlying `Object` and ties the handle to the N-API
/// handle scope it was obtained in, exactly like any other napi-rs JS value.
pub struct JsInstance<'env, C> {
    object: Object<'env>,
    _class: PhantomData<C>,
}

impl<'env, C> JsInstance<'env, C> {
    /// Tags an already-constructed `Object` as an instance of class `C`.
    /// The caller is responsible for ensuring `object` really is an instance of `C`.
    pub(crate) fn from_object(object: Object<'env>) -> Self {
        Self {
            object,
            _class: PhantomData,
        }
    }

    /// The raw `napi_env` this handle was created in.
    ///
    /// This is the only way to determine which N-API environment a `JsInstance` actually belongs
    /// to. Callers that hand a `JsInstance` to an API expecting a specific `Env` must compare
    /// this against that `Env`'s `env.raw()` themselves.
    pub(crate) fn env_raw(&self) -> sys::napi_env {
        self.object.value().env
    }
}

// A `JsInstance` is just a handle, like the `Object` it wraps, so copying it is free and creates
// no new GC root. Derives are avoided here: they would needlessly require `C: Clone`/`C: Copy`,
// which the zero-sized class markers do not implement.
impl<C> Clone for JsInstance<'_, C> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<C> Copy for JsInstance<'_, C> {}

impl<C> ToNapiValue for JsInstance<'_, C> {
    unsafe fn to_napi_value(env: sys::napi_env, val: Self) -> napi::Result<sys::napi_value> {
        assert_eq!(
            val.object.value().env,
            env,
            "JsInstance::to_napi_value called with env different from the one the value belongs to"
        );
        unsafe { ToNapiValue::to_napi_value(env, val.object) }
    }
}
