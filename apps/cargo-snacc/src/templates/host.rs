mod interop;

#[cfg(snacc_bridge_assertions)]
include!(env!("SNACC_BRIDGE_ASSERTIONS"));

use ferris_says::say;
use std::io::{stdout, BufWriter};

unsafe extern "C" {
    fn snacc_main() -> i32;
}

fn main() {
    let stdout = stdout();
    let writer = BufWriter::new(stdout.lock());
    say("Hello from a Snacc application!", 32, writer)
        .expect("ferris-says failed to write the demo");

    snacc_runtime::force_link();
    // SAFETY: cargo-snacc links this host with the object defining this ABI.
    let status = unsafe { snacc_main() };
    std::process::exit(status);
}

#[test]
fn snacc_entry_succeeds() {
    snacc_runtime::force_link();
    // SAFETY: cargo-snacc links this harness with the object defining this ABI.
    assert_eq!(unsafe { snacc_main() }, 0);
}
