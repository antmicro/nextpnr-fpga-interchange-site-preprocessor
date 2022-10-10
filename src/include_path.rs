/* Note:
 * This breaks cross-compilation. An alternative trick is to check the `cfg`
 * in `build.rs`, set `cargo:rust-cfg=` based on that and use that here, but this
 * is not recognized by rust-analyzer.
 */

#[cfg(unix)]
macro_rules! include_interchange_capnp {
    ($filename:literal) => {
        include!(concat!(env!("OUT_DIR"), "/interchange/", $filename));
    };
}

#[cfg(windows)]
macro_rules! include_interchange_capnp {
    ($filename:literal) => {
        include!(concat!(env!("OUT_DIR"), "\\interchange\\", $filename));
    };
}
