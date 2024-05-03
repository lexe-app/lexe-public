//! `std::sync::LazyLock`, copied from `std` so it's available outside nightly.
//!
//! TODO(phlip9): remove this once `LazyLock` stabilizes

use std::{
    cell::UnsafeCell,
    fmt,
    mem::ManuallyDrop,
    ops::Deref,
    panic::{RefUnwindSafe, UnwindSafe},
    sync::Once,
};

// We use the state of a Once as discriminant value. Upon creation, the state is
// "incomplete" and `f` contains the initialization closure. In the first call
// to `call_once`, `f` is taken and run. If it succeeds, `value` is set and the
// state is changed to "complete". If it panics, the Once is poisoned, so none
// of the two fields is initialized.
union Data<T, F> {
    value: ManuallyDrop<T>,
    f: ManuallyDrop<F>,
}

/// A value which is initialized on the first access.
///
/// This type is a thread-safe [`LazyCell`], and can be used in statics.
/// Since initialization may be called from multiple threads, any
/// dereferencing call will block the calling thread if another
/// initialization routine is currently running.
///
/// [`LazyCell`]: std::cell::LazyCell
pub struct LazyLock<T, F = fn() -> T> {
    once: Once,
    data: UnsafeCell<Data<T, F>>,
}

impl<T, F: FnOnce() -> T> LazyLock<T, F> {
    /// Creates a new lazy value with the given initializing function.
    #[inline]
    pub const fn new(f: F) -> LazyLock<T, F> {
        LazyLock {
            once: Once::new(),
            data: UnsafeCell::new(Data {
                f: ManuallyDrop::new(f),
            }),
        }
    }

    /// Forces the evaluation of this lazy value and returns a reference to
    /// result. This is equivalent to the `Deref` impl, but is explicit.
    ///
    /// This method will block the calling thread if another initialization
    /// routine is currently running.
    #[inline]
    pub fn force(this: &LazyLock<T, F>) -> &T {
        this.once.call_once(|| {
            // SAFETY: `call_once` only runs this closure once, ever.
            let data = unsafe { &mut *this.data.get() };
            let f = unsafe { ManuallyDrop::take(&mut data.f) };
            let value = f();
            data.value = ManuallyDrop::new(value);
        });

        // SAFETY:
        // There are four possible scenarios:
        // * the closure was called and initialized `value`.
        // * the closure was called and panicked, so this point is never
        //   reached.
        // * the closure was not called, but a previous call initialized
        //   `value`.
        // * the closure was not called because the Once is poisoned, so this
        //   point is never reached.
        // So `value` has definitely been initialized and will not be modified
        // again.
        unsafe { &(*this.data.get()).value }
    }
}

impl<T, F> LazyLock<T, F> {
    /// Get the inner value if it has already been initialized.
    fn get(&self) -> Option<&T> {
        if self.once.is_completed() {
            // SAFETY:
            // The closure has been run successfully, so `value` has been
            // initialized and will not be modified again.
            Some(unsafe { &*(*self.data.get()).value })
        } else {
            None
        }
    }
}

impl<T, F> Drop for LazyLock<T, F> {
    fn drop(&mut self) {
        if self.once.is_completed() {
            unsafe { ManuallyDrop::drop(&mut self.data.get_mut().value) }
        } else {
            // phlip9: if we `panic!` during init, the `LazyLock` is poisoned
            // and we can't guarantee `self.data` contains the `f` variant. Just
            // leak in this case (better than unsafety).
            // Note: std impl properly frees the closure, but uses non-pub APIs.
        }
    }
}

impl<T, F: FnOnce() -> T> Deref for LazyLock<T, F> {
    type Target = T;

    /// Dereferences the value.
    ///
    /// This method will block the calling thread if another initialization
    /// routine is currently running.
    #[inline]
    fn deref(&self) -> &T {
        LazyLock::force(self)
    }
}

impl<T: Default> Default for LazyLock<T> {
    /// Creates a new lazy value using `Default` as the initializing function.
    #[inline]
    fn default() -> LazyLock<T> {
        LazyLock::new(T::default)
    }
}

impl<T: fmt::Debug, F> fmt::Debug for LazyLock<T, F> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut d = f.debug_tuple("LazyLock");
        match self.get() {
            Some(v) => d.field(v),
            None => d.field(&format_args!("<uninit>")),
        };
        d.finish()
    }
}

// We never create a `&F` from a `&LazyLock<T, F>` so it is fine
// to not impl `Sync` for `F`.
unsafe impl<T: Sync + Send, F: Send> Sync for LazyLock<T, F> {}
// auto-derived `Send` impl is OK.

impl<T: RefUnwindSafe + UnwindSafe, F: UnwindSafe> RefUnwindSafe
    for LazyLock<T, F>
{
}
impl<T: UnwindSafe, F: UnwindSafe> UnwindSafe for LazyLock<T, F> {}
