fn main() {
    if let Some(libdir) = rb_sys_build::rb_config().get("libdir") {
        println!("cargo:rustc-link-arg=-Wl,-rpath,{libdir}");
    }
}
