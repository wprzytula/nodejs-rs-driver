/// A `napi::bindgen_prelude::Reference<T>` (and other N-API handles built on top of it) can only
/// ever be safely created, read, or dropped on the thread that owns the JS engine it was created
/// for. Because of that, `Reference<T>` is (rightfully) not `Send`.
///
/// However, some wrapper types (e.g. `SessionWrapper`) also expose `async` methods, and for those
/// to compile, `&Self` must be `Send`, which in turn requires every field of `Self` to be `Sync`.
/// This wrapper asserts `Send`/`Sync` to satisfy the compiler when carrying fields that do not
/// fulfill those requirements.
///
/// The wrapped value is kept private to this module: nothing outside of it can construct a
/// `JsThreadOnly` or reach into the value it holds except through the accessors below. That keeps
/// the safety invariant auditable in a single place instead of at every call site.
///
/// # Safety
/// Values of this type must only be constructed, read, or dropped from the JS thread
/// (i.e. from within a synchronous `#[napi]` function, or a finalizer callback - both of which
/// N-API always runs on the JS thread). Do not construct, read, or drop this from within an
/// `async` method (across an `.await` point, control may resume on a different thread).
pub(crate) struct JsThreadOnly<T>(T);

unsafe impl<T> Send for JsThreadOnly<T> {}
unsafe impl<T> Sync for JsThreadOnly<T> {}

impl<T> JsThreadOnly<T> {
    /// Wraps `value`, asserting that it (and everything reachable from it) will only ever be
    /// touched from the JS thread.
    ///
    /// # Safety
    /// The caller must be on the JS thread. See the type-level safety comment.
    pub(crate) fn new(value: T) -> Self {
        JsThreadOnly(value)
    }

    /// Borrows the wrapped value.
    ///
    /// # Safety
    /// The caller must be on the JS thread. See the type-level safety comment.
    pub(crate) fn get(&self) -> &T {
        &self.0
    }

    /// Mutably borrows the wrapped value.
    ///
    /// # Safety
    /// The caller must be on the JS thread. See the type-level safety comment.
    pub(crate) fn get_mut(&mut self) -> &mut T {
        &mut self.0
    }
}
