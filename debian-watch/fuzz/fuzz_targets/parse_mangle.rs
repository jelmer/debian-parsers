#![no_main]

use debian_watch::mangle::{parse_mangle_expr, parse_subst_expr, parse_transl_expr};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|s: &str| {
    let _ = parse_mangle_expr(s);
    let _ = parse_subst_expr(s);
    let _ = parse_transl_expr(s);
});
