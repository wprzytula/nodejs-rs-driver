//! JS-side tests for the `define_js_callback!` macro (`crate::utils::js_ctor`) and
//! `crate::utils::js_fn_registry`, exercised through a test-only `testAppend(name, value)`
//! callback.
//!
//! The interesting behaviour is what happens when the JS callback throws part-way through an
//! iteration: `for_each` must stop there and hand the exception back, rather than carrying on and
//! silently dropping it - the same first-error-wins contract the C# driver gets from
//! `FFIMaybeException`.

use napi::Env;
use napi::bindgen_prelude::FnArgs;

use crate::errors::{ConvertedResult, JsResult, with_custom_error_sync};
use crate::utils::js_ctor::borrow_test_append_callback;

/// Streams `count` items through the registered `testAppend` callback, one at a time.
///
/// Returns how many items were actually handed over before the callback threw (or `count`, if it
/// never did). A JS exception aborts the iteration and is propagated, so the count observed on the
/// JS side after catching it tells us exactly where iteration stopped.
#[napi(ts_return_type = "void")]
pub fn tests_stream_to_append_callback(count: u32, env: &Env) -> JsResult<()> {
    with_custom_error_sync(|| {
        let append = borrow_test_append_callback(env)?;
        append.for_each((0..count).map(|i| FnArgs::from(("item", i as i32))))?;
        ConvertedResult::Ok(())
    })
}
