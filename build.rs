fn main() {
    #[cfg(feature = "__testing")]
    println!("cargo:rustc-cfg=test");
}
