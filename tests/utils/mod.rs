pub mod race_checker;

macro_rules! assert_is_trait {
    ($obj:ty, $trait:path) => {{
        fn is_trait<T: $trait>() {}
        is_trait::<$obj>();
    }};
    ($obj:ty, $($pos:path),* $(,)?) => {
        $(assert_is_trait!($obj, $pos);)*
    };

    ($obj:ty, !$trait:path) => {{
        struct BansThisImpl<T: ?Sized>(core::marker::PhantomData<T>);

        impl<T: ?Sized + $trait> BansThisImpl<T> {
            #[forbid(
                unfulfilled_lint_expectations,
                reason = "This lint expectation is attempting a trait assertion, but it failed: \
                (Expected Object to not implement Trait but it implements it.)"
            )]
            #[expect(dead_code)]
            const SATISFIED: bool = panic!("Assertion failed: Forbidden trait bound.");
        }

        #[forbid(
            dead_code,
            reason = "This lint expectation is attempting a trait assertion, but it failed: \
            (Expected Object to not implement Trait but it implements it.)"
        )]
        trait AllowsOtherImpls {
            const SATISFIED: bool = true;
        }
        impl<T: ?Sized> AllowsOtherImpls for BansThisImpl<T> {}

        // A bit of a hack. We disambiguate between `AllowsOtherImpls`'s and `BansThisImpl`'s
        //  `SATISFIED` to determine if we're implementing the trait or not.
        // See https://stackoverflow.com/questions/71720817/check-if-a-trait-is-implemented-or-not
        // and https://users.rust-lang.org/t/check-if-a-trait-is-implemented-or-not/73756
        assert!(<BansThisImpl<$obj>>::SATISFIED);
    }};
    ($obj:ty, $(!$neg:path),*) => {
        $(assert_is_trait!($obj, !$neg);)*
    };
}

pub(super) use assert_is_trait;
