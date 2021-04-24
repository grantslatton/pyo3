use crate::{
    exceptions, FromPyObject, IntoPy, PyAny, PyErr, PyObject, PyResult, PyTryFrom, Python,
    ToPyObject,
};

#[cfg(not(min_const_generics))]
macro_rules! array_impls {
    ($($N:expr),+) => {
        $(
            impl<T> IntoPy<PyObject> for [T; $N]
            where
                T: ToPyObject
            {
                fn into_py(self, py: Python) -> PyObject {
                    self.as_ref().to_object(py)
                }
            }

            impl<'a, T> FromPyObject<'a> for [T; $N]
            where
                T: Copy + Default + FromPyObject<'a>,
            {
                #[cfg(not(feature = "nightly"))]
                fn extract(obj: &'a PyAny) -> PyResult<Self> {
                    let mut array = [T::default(); $N];
                    _extract_sequence_into_slice(obj, &mut array)?;
                    Ok(array)
                }

                #[cfg(feature = "nightly")]
                default fn extract(obj: &'a PyAny) -> PyResult<Self> {
                    let mut array = [T::default(); $N];
                    _extract_sequence_into_slice(obj, &mut array)?;
                    Ok(array)
                }
            }

            #[cfg(feature = "nightly")]
            impl<'source, T> FromPyObject<'source> for [T; $N]
            where
                for<'a> T: Default + FromPyObject<'a> + crate::buffer::Element,
            {
                fn extract(obj: &'source PyAny) -> PyResult<Self> {
                    let mut array = [T::default(); $N];
                    // first try buffer protocol
                    if unsafe { crate::ffi::PyObject_CheckBuffer(obj.as_ptr()) } == 1 {
                        if let Ok(buf) = crate::buffer::PyBuffer::get(obj) {
                            if buf.dimensions() == 1 && buf.copy_to_slice(obj.py(), &mut array).is_ok() {
                                buf.release(obj.py());
                                return Ok(array);
                            }
                            buf.release(obj.py());
                        }
                    }
                    // fall back to sequence protocol
                    _extract_sequence_into_slice(obj, &mut array)?;
                    Ok(array)
                }
            }
        )+
    }
}

#[cfg(not(min_const_generics))]
array_impls!(
    0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24, 25,
    26, 27, 28, 29, 30, 31, 32
);

#[cfg(min_const_generics)]
impl<T, const N: usize> IntoPy<PyObject> for [T; N]
where
    T: ToPyObject,
{
    fn into_py(self, py: Python) -> PyObject {
        self.as_ref().to_object(py)
    }
}

#[cfg(min_const_generics)]
impl<'a, T, const N: usize> FromPyObject<'a> for [T; N]
where
    T: FromPyObject<'a>,
{
    #[cfg(not(feature = "nightly"))]
    fn extract(obj: &'a PyAny) -> PyResult<Self> {
        create_array_from_obj(obj)
    }

    #[cfg(feature = "nightly")]
    default fn extract(obj: &'a PyAny) -> PyResult<Self> {
        create_array_from_obj(obj)
    }
}

#[cfg(all(min_const_generics, feature = "nightly"))]
impl<'source, T, const N: usize> FromPyObject<'source> for [T; N]
where
    for<'a> T: Default + FromPyObject<'a> + crate::buffer::Element,
{
    fn extract(obj: &'source PyAny) -> PyResult<Self> {
        use crate::{AsPyPointer, PyNativeType};
        let mut array = [T::default(); N];
        // first try buffer protocol
        if unsafe { crate::ffi::PyObject_CheckBuffer(obj.as_ptr()) } == 1 {
            if let Ok(buf) = crate::buffer::PyBuffer::get(obj) {
                if buf.dimensions() == 1 && buf.copy_to_slice(obj.py(), &mut array).is_ok() {
                    buf.release(obj.py());
                    return Ok(array);
                }
                buf.release(obj.py());
            }
        }
        // fall back to sequence protocol
        _extract_sequence_into_slice(obj, &mut array)?;
        Ok(array)
    }
}

#[cfg(min_const_generics)]
fn create_array_from_obj<'s, T, const N: usize>(obj: &'s PyAny) -> PyResult<[T; N]>
where
    T: FromPyObject<'s>,
{
    let seq = <crate::types::PySequence as PyTryFrom>::try_from(obj)?;
    let expected_len = seq.len()? as usize;
    array_try_from_fn(|idx| {
        seq.get_item(idx as isize)
            .map_err(|_| invalid_sequence_length(expected_len, idx + 1))?
            .extract::<T>()
    })
}

// TODO use std::array::try_from_fn, if that stabilises:
// (https://github.com/rust-lang/rust/pull/75644)
#[cfg(min_const_generics)]
fn array_try_from_fn<E, F, T, const N: usize>(mut cb: F) -> Result<[T; N], E>
where
    F: FnMut(usize) -> Result<T, E>,
{
    // Helper to safely create arrays since the standard library doesn't
    // provide one yet. Shouldn't be necessary in the future.
    struct ArrayGuard<T, const N: usize> {
        dst: *mut T,
        initialized: usize,
    }

    impl<T, const N: usize> Drop for ArrayGuard<T, N> {
        fn drop(&mut self) {
            debug_assert!(self.initialized <= N);
            let initialized_part = core::ptr::slice_from_raw_parts_mut(self.dst, self.initialized);
            unsafe {
                core::ptr::drop_in_place(initialized_part);
            }
        }
    }

    // [MaybeUninit<T>; N] would be "nicer" but is actually difficult to create - there are nightly
    // APIs which would make this easier.
    let mut array: core::mem::MaybeUninit<[T; N]> = core::mem::MaybeUninit::uninit();
    let mut guard: ArrayGuard<T, N> = ArrayGuard {
        dst: array.as_mut_ptr() as _,
        initialized: 0,
    };
    unsafe {
        let mut value_ptr = array.as_mut_ptr() as *mut T;
        for i in 0..N {
            core::ptr::write(value_ptr, cb(i)?);
            value_ptr = value_ptr.offset(1);
            guard.initialized += 1;
        }
        core::mem::forget(guard);
        Ok(array.assume_init())
    }
}

fn _extract_sequence_into_slice<'s, T>(obj: &'s PyAny, slice: &mut [T]) -> PyResult<()>
where
    T: FromPyObject<'s>,
{
    let seq = <crate::types::PySequence as PyTryFrom>::try_from(obj)?;
    let expected_len = seq.len()? as usize;
    if expected_len != slice.len() {
        return Err(invalid_sequence_length(expected_len, slice.len()));
    }
    for (value, item) in slice.iter_mut().zip(seq.iter()?) {
        *value = item?.extract::<T>()?;
    }
    Ok(())
}

pub fn invalid_sequence_length(expected: usize, actual: usize) -> PyErr {
    exceptions::PyValueError::new_err(format!(
        "expected a sequence of length {} (got {})",
        expected, actual
    ))
}

#[cfg(test)]
mod test {
    use crate::Python;
    #[cfg(min_const_generics)]
    use std::{
        panic,
        sync::atomic::{AtomicUsize, Ordering},
    };

    #[cfg(min_const_generics)]
    #[test]
    fn array_try_from_fn() {
        static DROP_COUNTER: AtomicUsize = AtomicUsize::new(0);
        struct CountDrop;
        impl Drop for CountDrop {
            fn drop(&mut self) {
                DROP_COUNTER.fetch_add(1, Ordering::SeqCst);
            }
        }
        let _ = catch_unwind_silent(move || {
            let _: Result<[CountDrop; 4], ()> = super::array_try_from_fn(|idx| {
                if idx == 2 {
                    panic!("peek a boo");
                }
                Ok(CountDrop)
            });
        });
        assert_eq!(DROP_COUNTER.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn test_extract_small_bytearray_to_array() {
        let gil = Python::acquire_gil();
        let py = gil.python();
        let v: [u8; 3] = py
            .eval("bytearray(b'abc')", None, None)
            .unwrap()
            .extract()
            .unwrap();
        assert!(&v == b"abc");
    }

    #[cfg(min_const_generics)]
    #[test]
    fn test_extract_bytearray_to_array() {
        let gil = Python::acquire_gil();
        let py = gil.python();
        let v: [u8; 33] = py
            .eval(
                "bytearray(b'abcabcabcabcabcabcabcabcabcabcabc')",
                None,
                None,
            )
            .unwrap()
            .extract()
            .unwrap();
        assert!(&v == b"abcabcabcabcabcabcabcabcabcabcabc");
    }

    // https://stackoverflow.com/a/59211505
    #[cfg(min_const_generics)]
    fn catch_unwind_silent<F, R>(f: F) -> std::thread::Result<R>
    where
        F: FnOnce() -> R + panic::UnwindSafe,
    {
        let prev_hook = panic::take_hook();
        panic::set_hook(Box::new(|_| {}));
        let result = panic::catch_unwind(f);
        panic::set_hook(prev_hook);
        result
    }
}
