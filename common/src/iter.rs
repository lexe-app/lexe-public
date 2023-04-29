#![allow(clippy::wrong_self_convention)]

use std::cmp;

/// [`Iterator`] extension trait
pub trait IteratorExt: Iterator {
    /// Returns `true` iff the iterator is a strict total order. This implies
    /// the iterator is sorted and all elements are unique.
    ///
    /// ```ignore
    /// [x_1, ..., x_n].is_strict_total_order()
    ///     := x_1 < x_2 < ... < x_n
    /// ```
    ///
    /// ### Examples
    ///
    /// ```rust
    /// use std::iter;
    /// use common::iter::IteratorExt;
    ///
    /// assert!(iter::empty::<u32>().is_strict_total_order());
    /// assert!(&[1].iter().is_strict_total_order());
    /// assert!(&[1, 2, 6].iter().is_strict_total_order());
    ///
    /// assert!(!&[2, 1].iter().is_strict_total_order());
    /// assert!(!&[1, 2, 2, 3].iter().is_strict_total_order());
    /// ```
    fn is_strict_total_order(mut self) -> bool
    where
        Self: Sized,
        Self::Item: PartialOrd,
    {
        let mut prev = match self.next() {
            Some(first) => first,
            // Trivially true
            None => return true,
        };

        for next in self {
            if let Some(cmp::Ordering::Greater)
            | Some(cmp::Ordering::Equal)
            | None = prev.partial_cmp(&next)
            {
                return false;
            }
            prev = next;
        }

        true
    }

    /// Returns `true` iff the iterator is a strict total order according to the
    /// key extraction function `f`.
    ///
    /// ```ignore
    /// [x_1, ..., x_n].is_strict_total_order_by_key(f)
    ///     := f(x_1) < f(x_2) < ... < f(x_n)
    /// ```
    fn is_strict_total_order_by_key<F, K>(self, f: F) -> bool
    where
        Self: Sized,
        F: FnMut(Self::Item) -> K,
        K: PartialOrd,
    {
        self.map(f).is_strict_total_order()
    }
}
impl<I: Iterator> IteratorExt for I {}
