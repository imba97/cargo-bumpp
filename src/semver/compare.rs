//! Pre-release precedence: how `-alpha`, `-alpha.1`, `-1` and no pre-release at
//! all order against each other.
//!
//! It is its own file because it is the one rule every comparison depends on,
//! and it is subtle enough (numeric identifiers sort below alphanumeric ones, a
//! shorter list sorts first, no pre-release sorts highest) to want one home.

use std::cmp::Ordering;

use super::version::PreId;

pub(super) fn compare_pre(a: &[PreId], b: &[PreId]) -> Ordering {
    match (a.is_empty(), b.is_empty()) {
        // A version without pre-release has higher precedence.
        (true, true) => Ordering::Equal,
        (true, false) => Ordering::Greater,
        (false, true) => Ordering::Less,
        (false, false) => {
            for (left, right) in a.iter().zip(b.iter()) {
                let ord = match (left, right) {
                    (PreId::Num(x), PreId::Num(y)) => x.cmp(y),
                    (PreId::Num(_), PreId::Alpha(_)) => Ordering::Less,
                    (PreId::Alpha(_), PreId::Num(_)) => Ordering::Greater,
                    (PreId::Alpha(x), PreId::Alpha(y)) => x.cmp(y),
                };
                if ord != Ordering::Equal {
                    return ord;
                }
            }
            a.len().cmp(&b.len())
        }
    }
}
