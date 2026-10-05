//! CPython extension module `kvzip._native`; `python/kvzip` re-exports and types it.

use std::path::PathBuf;

use pyo3::{
    exceptions::{PyOSError, PyRuntimeError, PyValueError},
    prelude::*,
    pybacked::{PyBackedBytes, PyBackedStr},
    types::PyBytes,
};

/// A key or a value; a `str` stands for its UTF-8 encoding.
#[derive(FromPyObject)]
enum Data {
    Bytes(PyBackedBytes),
    Str(PyBackedStr),
}

impl Data {
    fn as_bytes(&self) -> &[u8] {
        match self {
            Data::Bytes(bytes) => bytes,
            Data::Str(text) => text.as_bytes(),
        }
    }
}

#[pyclass(frozen, module = "kvzip")]
struct Store {
    inner: kvzip::Store,
}

// Every method that touches the disk detaches from the interpreter so that other Python
// threads run meanwhile.
#[pymethods]
impl Store {
    #[new]
    #[pyo3(signature = (directory, *, max_segment_bytes = kvzip::DEFAULT_MAX_SEGMENT_BYTES))]
    fn new(py: Python<'_>, directory: PathBuf, max_segment_bytes: u64) -> PyResult<Self> {
        let options = kvzip::Options { max_segment_bytes };
        let inner = py
            .detach(|| kvzip::Store::open(directory, options))
            .map_err(to_py_error)?;
        Ok(Self { inner })
    }

    fn get<'py>(&self, py: Python<'py>, key: Data) -> PyResult<Option<Bound<'py, PyBytes>>> {
        let value = py
            .detach(|| self.inner.get(key.as_bytes()))
            .map_err(to_py_error)?;
        Ok(value.map(|value| PyBytes::new(py, &value)))
    }

    fn put(&self, py: Python<'_>, key: Data, value: Data) -> PyResult<()> {
        py.detach(|| self.inner.put(key.as_bytes(), value.as_bytes()))
            .map_err(to_py_error)
    }

    fn __contains__(&self, key: Data) -> bool {
        self.inner.contains(key.as_bytes())
    }

    fn __len__(&self) -> usize {
        self.inner.len()
    }

    fn keys<'py>(&self, py: Python<'py>) -> Vec<Bound<'py, PyBytes>> {
        let keys = self.inner.keys();
        keys.iter().map(|key| PyBytes::new(py, key)).collect()
    }

    fn refresh(&self, py: Python<'_>) -> PyResult<()> {
        py.detach(|| self.inner.refresh()).map_err(to_py_error)
    }
}

fn to_py_error(error: kvzip::Error) -> PyErr {
    let message = error.to_string();
    match error {
        kvzip::Error::Io(_) => PyOSError::new_err(message),
        kvzip::Error::RecordTooLarge { .. } | kvzip::Error::InvalidOptions(_) => {
            PyValueError::new_err(message)
        }
        kvzip::Error::Corrupt(_) => PyRuntimeError::new_err(message),
    }
}

#[pymodule]
fn _native(module: &Bound<'_, PyModule>) -> PyResult<()> {
    module.add_class::<Store>()
}
