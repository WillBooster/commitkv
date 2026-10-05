//! Node-API addon; `src/index.ts` wraps it in the public TypeScript API.

use napi::bindgen_prelude::{Buffer, Error, Result};
use napi_derive::napi;

#[napi]
pub struct Store {
    inner: kvzip::Store,
}

#[napi]
impl Store {
    #[napi(constructor)]
    pub fn new(directory: String, max_segment_bytes: Option<f64>) -> Result<Self> {
        let mut options = kvzip::Options::default();
        if let Some(max_segment_bytes) = max_segment_bytes {
            // The cast saturates and maps NaN to 0, both of which the store rejects.
            options.max_segment_bytes = max_segment_bytes as u64;
        }
        let inner = kvzip::Store::open(directory, options).map_err(to_js_error)?;
        Ok(Self { inner })
    }

    #[napi]
    pub fn get(&self, key: &[u8]) -> Result<Option<Buffer>> {
        let value = self.inner.get(key).map_err(to_js_error)?;
        Ok(value.map(Buffer::from))
    }

    #[napi]
    pub fn put(&self, key: &[u8], value: &[u8]) -> Result<()> {
        self.inner.put(key, value).map_err(to_js_error)
    }

    #[napi]
    pub fn has(&self, key: &[u8]) -> bool {
        self.inner.contains(key)
    }

    #[napi(getter)]
    pub fn size(&self) -> f64 {
        self.inner.len() as f64
    }

    #[napi]
    pub fn keys(&self) -> Vec<Buffer> {
        self.inner.keys().into_iter().map(Buffer::from).collect()
    }

    #[napi]
    pub fn refresh(&self) -> Result<()> {
        self.inner.refresh().map_err(to_js_error)
    }
}

fn to_js_error(error: kvzip::Error) -> Error {
    Error::from_reason(error.to_string())
}
