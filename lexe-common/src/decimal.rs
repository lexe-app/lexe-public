use rust_decimal::Decimal;

/// A simpler, const-friendly, proc-macro-free `rust_decimal_macros::dec`.
///
/// Only handles decimals in the range `-2**63 <= x <= 2**63` to keep the
/// implementation simpler.
#[macro_export]
macro_rules! dec {
    ($amount:expr) => {
        const { $crate::decimal::decimal_from_str_const(stringify!($amount)) }
    };
}

/// A `const` version of [`Decimal::from_str_radix`] (with radix=10).
///
/// Only handles decimals in the range `-2**63 <= x <= 2**63` to keep the
/// implementation simpler.
pub const fn decimal_from_str_const(s: &str) -> Decimal {
    let mut bs = s.as_bytes();

    // check sign
    let mut negative = false;
    match bs.split_first() {
        Some((b, rest)) => match b {
            b'-' => {
                negative = true;
                bs = rest;
            }
            b'+' => bs = rest,
            _ => {}
        },
        None => panic!("empty"),
    }

    // parse the actual number, keeping track of the decimal position and
    // skipping any "_" characters.
    let mut num_digits: u8 = 0;
    let mut idx_point: Option<u8> = None;
    let mut accum: u64 = 0;
    while let [b, rest @ ..] = bs {
        bs = rest;
        match *b {
            b'0'..=b'9' => {
                let d = (*b - b'0') as u64;
                accum = accum.checked_mul(10).expect("overflow");
                accum = accum.checked_add(d).expect("overflow");
                num_digits += 1;
            }
            b'.' => {
                if idx_point.is_some() {
                    panic!("duplicate decimal point");
                }
                idx_point = Some(num_digits);
            }
            b'_' => continue,
            _ => panic!("not a valid decimal"),
        }
    }

    // probably an error
    if num_digits == 0 {
        panic!("no digits");
    }

    let lo = (accum & 0xffff_ffff) as u32;
    let mid = ((accum >> 32) & 0xffff_ffff) as u32;
    let hi = 0;
    let idx_point = match idx_point {
        Some(x) => x,
        None => num_digits,
    };
    let scale = (num_digits - idx_point) as u32;

    Decimal::from_parts(lo, mid, hi, negative, scale)
}

#[cfg(test)]
mod test {
    use proptest::proptest;

    use super::*;

    const MYCONST: Decimal = dec!(132.456);

    #[test]
    fn test_decimal_from_str_const() {
        #[track_caller]
        fn ok(s: &str) {
            let actual = decimal_from_str_const(s);
            let expected = Decimal::from_str_radix(s, 10).unwrap();
            assert_eq!(actual, expected);
        }

        ok("1");
        ok("1.");
        ok(".1");
        ok("-1");
        ok("0.1");
        ok("-0.1");
        ok("2.0");
        ok("-0");

        ok("9223372036854775808");
        ok("-9223372036854775808");

        ok("9.223372036854775808");
        ok("922337203685477580.8");
        ok("9223372036854775808.");

        assert_eq!(MYCONST, Decimal::from_parts(132456, 0, 0, false, 3));

        proptest!(|(x: u64, negative: bool, scale in 0u32..20)| {
            // sample only x < 2**63
            let x = x & 0x7fff_ffff_ffff_ffff;
            let lo = (x & 0xffff_ffff) as u32;
            let mid = ((x >> 32) & 0xffff_ffff) as u32;
            let hi = 0;
            let dec = Decimal::from_parts(lo, mid, hi, negative, scale);
            ok(&dec.to_string());
        })
    }
}
