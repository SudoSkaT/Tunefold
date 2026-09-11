//! Expone el target triple de compilación (`CARGO_CFG_*`/`TARGET`) como
//! variable de entorno en tiempo de compilación, para que el actualizador
//! ([`crate::app::updater`]) pueda descargar el asset correcto de una release.

fn main() {
    if let Ok(target) = std::env::var("TARGET") {
        println!("cargo:rustc-env=TUNEFOLD_TARGET={target}");
    }
}
