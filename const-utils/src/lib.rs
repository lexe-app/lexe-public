//! Utilities for use in `const` fns and expressions.

/// Assert at compile that that a boolean expression evaluates to true.
/// Implementation copied from the static_assertions crate.
#[macro_export]
macro_rules! const_assert {
    ($x:expr $(,)?) => {
        #[allow(clippy::const_is_empty, clippy::eq_op, unknown_lints)]
        const _: [(); 0 - !{
            const CONST_ASSERT: bool = $x;
            CONST_ASSERT
        } as usize] = [];
    };
}

/// Assert at compile time that two `usize` values are equal. This assert has a
/// nice benefit where there compiler error will actually _print out_ the
/// two values.
#[macro_export]
macro_rules! const_assert_usize_eq {
    ($x:expr, $y:expr $(,)?) => {
        const _: [(); $x] = [(); $y];
    };
}

/// Compile-time cast from `&T::From` to `&T`, where `T` is just a struct with
/// a single field of type `T::From` and `T` is `#[repr(transparent)]`.
///
/// Useful for casting a new-type's inner struct reference to a new-type
/// reference.
///
/// See [`ref_cast`] for more details. Just use `T::ref_cast` if you don't need
/// `const`.
///
/// ## Example
///
/// ```rust
/// use ref_cast::RefCast;
///
/// #[derive(RefCast)]
/// #[repr(transparent)]
/// struct Id(u32);
///
/// // Safe, const cast from `&123` to `&Id(123)`
/// const MY_ID: &'static Id = const_utils::const_ref_cast(&123);
/// ```
pub const fn const_ref_cast<T: ref_cast::RefCast>(from: &T::From) -> &T {
    // SAFETY: we require that `T: RefCast`, which guarantees that this cast is
    // safe. Unfortunately we need this extra method as `T::ref_cast` is not
    // currently const (Rust doesn't support const traits yet).
    unsafe { &*(from as *const T::From as *const T) }
}

/// [`Option::unwrap`] but works in `const fn`.
// TODO(phlip9): remove this when const unwrap stabilizes
pub const fn const_option_unwrap<T: Copy>(option: Option<T>) -> T {
    match option {
        Some(value) => value,
        None => panic!("unwrap on None"),
    }
}
