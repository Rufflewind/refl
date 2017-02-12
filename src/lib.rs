//! Provides the ability to imprint values at the type level, enabling
//! compile-time validation of values that only exist at run time.
//!
//! *Heavily inspired by Edward Kmett's [`reflection`][reflection] and
//! [`eq`][eq] libraries, as well as Gankro's [sound unchecked
//! indexing][sound] approach.*
//!
//! [reflection]: https://hackage.haskell.org/package/reflection
//! [eq]: https://hackage.haskell.org/package/eq
//! [sound]: https://reddit.com/r/rust/comments/3oo0oe

use std::borrow::Borrow;
use std::cell::Cell;
use std::marker::PhantomData;
use std::{fmt, mem};

/// Like `PhantomData` but ensures that `T` is always invariant.
pub type PhantomInvariantData<T> = PhantomData<*mut T>;

/// Like `PhantomData` but ensures that `'a` is always invariant.
pub type PhantomInvariantLifetime<'a> = PhantomData<Cell<&'a mut ()>>;

/// Imprint the type of an object with its own value.
///
/// A value of type `Self` is imprinted as `Val<'x, Self>`, where `'x` is
/// a unique marker for this particular value.  The callback receives the
/// value as its argument.
///
/// Note that the callback isn't allowed to smuggle the imprinted value out of
/// the closure, thanks to the [higher-rank trait bound][hrtb].
///
/// [hrtb]: https://doc.rust-lang.org/nomicon/hrtb.html
///
/// See [`Val`](struct.Val.html) for more information.
///
/// ```
/// # /*
/// fn imprint(T, impl for<'x> FnOnce(Val<'x, T>) -> R) -> R
/// # */
/// ```
///
/// ## Example
///
/// ```
/// use imprint::{Val, imprint};
///
/// imprint(42, |n: Val<i64>| {
///     assert_eq!(n.into_inner(), 42);
/// })
/// ```
pub fn imprint<F, R, T>(value: T, callback: F) -> R
    where F: for<'x> FnOnce(Val<'x, T>) -> R {
    callback(Val { tag: PhantomData, inner: value })
}

/// A value marked at the type level.
///
/// A `Val<'x, T>` value contains an instance of `T` as well as a marker
/// `'x` that reflects the value of that instance at the type level.  This
/// provides a type-safe mechanism to constrain values even if their actual
/// values are not known at compile time.
///
/// If the underlying type `T` is `PartialEq`-pure, then for every marker
/// `'x`, the type `Val<'x, T>` contains precisely one value.  Hence,
/// `Val<'x, T>` may be considered a *singleton type* (unrelated to
/// "singletons" in OOP).
///
/// A type `T` is said to be `PartialEq`-pure if, given any `r1: &T` and `r2:
/// &T`, and any arbitrary *unsafe-free* function `f(&T, &T)`, the changing
/// the evaluation order between `r1 == r2` and `f(r1, r2)` does not alter the
/// observable behavior of the program.  In other words, `Eq`-pure means that
/// every use of immutable references preserves equality.
///
/// Note: An *unsafe-free* function is a function that does not call any other
/// unsafe functions directly.  (TODO: How to handle modular boundary?)
///
/// Usually, anything that contains `Cell` or `RefCell` are not `*`-pure.
///
/// `Val` values can be constructed using either
/// [`imprint(...)`](fn.imprint.html) or `Default::default()`.
///
/// The underlying value can be extracted using `.into_inner()`, `.borrow()`,
/// or `.borrow_mut()`.
///
#[derive(Clone, Copy, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct Val<'x, T> {
    tag: PhantomInvariantLifetime<'x>,
    inner: T,
}

impl<'x, T: PartialEq> Val<'x, T> {
    /// Extracts the value.
    pub fn into_inner(self) -> T {
        self.inner
    }

    /// Checks whether two values are equal.  If they are, evidence of their
    /// type equality is returned.
    pub fn eq<'y>(&self, other: &Val<'y, T>)
                  -> Option<TyEq<Self, Val<'y, T>>> {
        let self_value: &T = self.borrow();
        if self_value == other.borrow() {
            Some(unsafe { mem::transmute(TyEq::<Self, Self>::refl()) })
        } else {
            None
        }
    }
}

impl<'x, T: Copy + fmt::Debug> fmt::Debug for Val<'x, T> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str("Val(")?;
        self.fmt(f)?;
        f.write_str(")")
    }
}

/// The default value always has the special marker of `'static`.
impl<T: Default> Default for Val<'static, T> {
    fn default() -> Self {
        Val { tag: PhantomData, inner: Default::default() }
    }
}

impl<'x, T> AsRef<T> for Val<'x, T> {
    fn as_ref(&self) -> &T {
        self.borrow()
    }
}

impl<'x, T> Borrow<T> for Val<'x, T> {
    fn borrow(&self) -> &T {
        &self.inner
    }
}

/// Propositional equality between types.
///
/// If two types `A` and `B` are equal, then it is safe to transmute between
/// `A` and `B` as well as any types that contain `A` or `B`.  The converse is
/// generally *not* true.
///
/// ## Unsafe: Conjuring equality out of thin air
///
/// It is sometimes useful to bypass Rust's type system to create a `TyEq<T,
/// U>` object where `T` is not *judgmentally* equal to `U`.  This can be done
/// by transmutation:
///
/// ```
/// # use imprint::{TyEq, PhantomInvariantLifetime};
/// # struct Foo<'a>(PhantomInvariantLifetime<'a>);
/// # unsafe fn conjure<'a, 'b>() -> TyEq<Foo<'a>, Foo<'b>> {
/// std::mem::transmute::<TyEq<Foo<'a>, Foo<'a>>,
///                       TyEq<Foo<'a>, Foo<'b>>>(TyEq::refl())
/// # }
/// ```
///
/// However, you must be absolutely certain that
///
///   - `A` and `B` are truly transmute-compatible (which usually means `A`
///     and `B` must differ only in phantom parameters), and
///   - changing from `A` to `B` or vice versa cannot alter the observable
///     behavior of any valid program.
///
/// The second condition is crucial: it is never correct equate two fully
/// concrete types (e.g. between `PhantomData<i64>` and `PhantomData<u64>`)
/// even if they are representationally identical, because one can always use
/// traits to dispatch based on the identity of the types, resulting in
/// differences in observable behavior.
///
/// Generally, it is only sensible to equate (partially) abstract types
/// (e.g. `Foobar<T>` and `Foobar<U>` where `T` and `U` are unknown), and even
/// still you have to make sure that this wouldn't cause changes in observable
/// behavior.  Most of the time, it only makes sense to equate generic phantom
/// lifetime parameters.
pub struct TyEq<T: ?Sized, U: ?Sized>(
    PhantomInvariantData<T>,
    PhantomInvariantData<U>,
);

impl<T: ?Sized> TyEq<T, T> {
    /// Constructor for `TyEq` (reflexivity).
    pub fn refl() -> Self {
        TyEq(PhantomData, PhantomData)
    }
}

impl<T: ?Sized, U: ?Sized> TyEq<T, U> {
    /// Substitute instances of `T` within a type with `U` (Leibniz's law,
    /// a.k.a. indiscernibility of identicals).
    ///
    /// The `apply` function allows you to freely convert between any two
    /// types as long as they differ only in `T` and `U`.  For example, you
    /// can turn `Vec<(T, T)>` into `Vec<(T, U)>`, `Vec<(U, T)>`, or
    /// `Vec<(U, U)>`.
    ///
    /// The type signature in the auto-generated documentation is unclear.
    /// It should've been more like:
    ///
    /// ```
    /// # /*
    /// fn apply(TyEq<T, U>, <F as TyFn<T>>::Output) -> <F as TyFn<U>>::Output
    /// # */
    /// ```
    ///
    /// In Haskell, it'd be simply `TyEq t u -> TyFn t f -> TyFn u f`.
    ///
    /// ## Example
    ///
    /// ```
    /// # use imprint::{TyEq, TyFn};
    /// // first define a type-level function using TyFn
    /// struct VecF;
    /// impl<T> TyFn<T> for VecF { type Output = Vec<T>; }
    ///
    /// // now we can convert from Vec<T> to Vec<U> as long as we have
    /// // TyEq<T, U> as evidence
    /// fn convert_vec<T, U>(eq: TyEq<T, U>, vec: Vec<T>) -> Vec<U> {
    ///     eq.apply::<VecF>(vec)
    /// }
    /// ```
    pub fn apply<F: ?Sized>(self, value: <F as TyFn<T>>::Output)
                            -> <F as TyFn<U>>::Output
        where F: TyFn<T> + TyFn<U>,
              <F as TyFn<T>>::Output: Sized,
              <F as TyFn<U>>::Output: Sized {
        // can't use transmute because the compiler isn't certain that the
        // sizes are equal (they *should* be equal, however)
        debug_assert_eq!(mem::size_of::<<F as TyFn<T>>::Output>(),
                         mem::size_of::<<F as TyFn<U>>::Output>());
        let result = unsafe { mem::transmute_copy(&value) };
        mem::forget(value);
        result
    }

    /// Exchange `T` and `U` (symmetry).
    pub fn sym(self) -> TyEq<U, T> {
        struct F<T: ?Sized>(PhantomInvariantData<T>);
        impl<T: ?Sized, U: ?Sized> TyFn<T> for F<U> {
            type Output = TyEq<T, U>;
        }
        self.apply::<F<T>>(TyEq::refl())
    }

    /// Compose two equalities (transitivity).
    pub fn trans<R: ?Sized>(self, other: TyEq<U, R>) -> TyEq<T, R> {
        struct F<T: ?Sized>(PhantomInvariantData<T>);
        impl<T: ?Sized, U: ?Sized> TyFn<T> for F<U> {
            type Output = TyEq<U, T>;
        }
        other.apply::<F<T>>(self)
    }
}

impl<T, U> TyEq<T, U> {
    /// Cast from `T` to `U`.
    ///
    /// Equivalent to <code>.<a href="#method.apply">apply</a>::&lt;<a
    /// href="struct.Identity.html">Identity</a>&gt;</code>.
    pub fn cast(self, value: T) -> U {
        self.apply::<Identity>(value)
    }
}

// shut up clippy: we don't want Clone constraints on T or U
#[cfg_attr(feature = "cargo-clippy", allow(expl_impl_clone_on_copy))]
impl<T: ?Sized, U: ?Sized> Clone for TyEq<T, U> {
    fn clone(&self) -> Self { *self }
}

impl<T: ?Sized, U: ?Sized> Copy for TyEq<T, U> { }

impl<T: ?Sized, U: ?Sized> fmt::Debug for TyEq<T, U> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str("TyEq")
    }
}

/// Used to implement type-level functions.
///
/// The parameter `F` identifies the type function and can be whatever you
/// want.  Note that `F` is the *main* parameter rather than an auxiliary
/// parameter: this allows users to implement their own type functions without
/// breaking the orphan rules.
///
/// ## Example
///
/// ```
/// # use imprint::TyFn;
/// // define a type function that converts T into Box<T>
/// struct BoxTyFn;
/// impl<T> TyFn<T> for BoxTyFn { type Output = Box<T>; }
/// ```
pub trait TyFn<F: ?Sized> {
    /// The result of the type function.
    type Output: ?Sized;
}

/// Identity function for types.
///
/// For all `T`, we have:
///
/// ```
/// # /*
/// <Identity as TyFn<T>>::Output == T
/// # */
/// ```
pub struct Identity;

impl<T: ?Sized> TyFn<T> for Identity {
    type Output = T;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn it_works() {
        imprint(42, |m| {
            assert_eq!(m.into_inner(), 42);
            let n = imprint(42, |n| {
                assert_eq!(n.into_inner(), 42);
                m.eq(&n).unwrap().sym().cast(n)
            });
            assert_eq!(m, n);
            imprint(0, |z| {
                assert_eq!(z.into_inner(), 0);
                assert!(m.eq(&z).is_none());
            })
        })
    }
}
