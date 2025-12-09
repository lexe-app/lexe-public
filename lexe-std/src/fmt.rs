use std::fmt;

/// Displays a slice of elements using each element's [`fmt::Display`] impl.
pub struct DisplaySlice<'a, T>(pub &'a [T]);

impl<T: fmt::Display> fmt::Display for DisplaySlice<'_, T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Slice iterators are cheaply cloneable (just pointer + length)
        DisplayIter(self.0.iter()).fmt(f)
    }
}

/// Displays an iterator of items using each element's [`fmt::Display`] impl.
///
/// As [`fmt::Display`] can't take ownership of the underlying iterator, the
/// iterator is cloned every time it is displayed, so it should be cheaply
/// clonable (most iterators are).
pub struct DisplayIter<I>(pub I);

impl<I> fmt::Display for DisplayIter<I>
where
    I: Iterator + Clone,
    I::Item: fmt::Display,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut first = true;
        write!(f, "[")?;
        for item in self.0.clone() {
            if !first {
                write!(f, ", ")?;
            }
            first = false;
            write!(f, "{item}")?;
        }
        write!(f, "]")
    }
}

/// Displays an iterator of items using each element's [`fmt::Debug`] impl.
///
/// As [`fmt::Display`] can't take ownership of the underlying iterator, the
/// iterator is cloned every time it is displayed, so it should be cheaply
/// clonable (most iterators are).
pub struct DebugIter<I>(pub I);

impl<I> fmt::Display for DebugIter<I>
where
    I: Iterator + Clone,
    I::Item: fmt::Debug,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut first = true;
        write!(f, "[")?;
        for item in self.0.clone() {
            if !first {
                write!(f, ", ")?;
            }
            first = false;
            write!(f, "{item:?}")?;
        }
        write!(f, "]")
    }
}
