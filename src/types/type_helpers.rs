use std::net::{IpAddr, SocketAddr};

use napi::{
    Env, JsValue,
    bindgen_prelude::{FromNapiValue, JsObjectValue, Object, ToNapiValue},
};

use crate::errors::make_js_error;

/// A type wrapper over `SocketAddr` to facilitate its usage over napi.
/// Can be created from net.SocketAddress object, or a duck-typed object with `address` and `port` fields.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SocketAddrWrapper {
    pub(crate) socket: SocketAddr,
}

impl FromNapiValue for SocketAddrWrapper {
    /// # Safety
    ///
    /// Valid pointer to napi env must be provided
    unsafe fn from_napi_value(
        env: napi::sys::napi_env,
        napi_val: napi::sys::napi_value,
    ) -> napi::Result<Self> {
        // Caller of this function ensures a valid pointer to napi env is provided
        let o = unsafe { ::napi::bindgen_prelude::Object::from_napi_value(env, napi_val) }?;

        let ip_str: String = o.get::<String>("address")?.ok_or_else(|| {
            make_js_error("Cannot retrieve socket address. Missing address field")
        })?;

        let port: u16 = o
            .get::<u16>("port")?
            .ok_or_else(|| make_js_error("Cannot retrieve socket address. Missing port field"))?;

        let ip: IpAddr = ip_str
            .parse()
            .map_err(|e| make_js_error(format!("Could not parse IP address: {}", e)))?;

        Ok(SocketAddrWrapper {
            socket: SocketAddr::new(ip, port),
        })
    }
}

impl ToNapiValue for SocketAddrWrapper {
    /// Produces the plain options object accepted by the `net.SocketAddress` constructor:
    /// `{ address, port, family }`.
    ///
    /// This is deliberately *not* a `net.SocketAddress` itself - building one requires calling that
    /// class's constructor, which is what `crate::utils::js_ctor::build_socket_address` does.
    /// Emitting the constructor's own input shape here keeps this impl the exact inverse of
    /// `FromNapiValue` above, so a `SocketAddrWrapper` round-trips through JS unchanged.
    ///
    /// `address` is the bare IP: no port, and for IPv6 no surrounding brackets (e.g. `::1`). That
    /// is what `net.SocketAddress` expects, and it differs from `SocketAddr`'s `Display`, which
    /// renders the combined `[::1]:9042` form.
    unsafe fn to_napi_value(
        env: napi::sys::napi_env,
        val: Self,
    ) -> napi::Result<napi::sys::napi_value> {
        let env = Env::from_raw(env);
        let mut obj = Object::new(&env)?;

        obj.set_named_property("address", val.socket.ip().to_string())?;
        obj.set_named_property("port", val.socket.port())?;
        obj.set_named_property(
            "family",
            match val.socket {
                SocketAddr::V4(_) => "ipv4",
                SocketAddr::V6(_) => "ipv6",
            },
        )?;

        Ok(obj.raw())
    }
}

impl SocketAddrWrapper {
    pub(crate) fn into_inner(self) -> SocketAddr {
        self.socket
    }
}

impl From<SocketAddr> for SocketAddrWrapper {
    fn from(socket: SocketAddr) -> Self {
        SocketAddrWrapper { socket }
    }
}
