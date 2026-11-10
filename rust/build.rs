// Incrusta el manifiesto de Windows y el ícono en el .exe.
// BX_ADMIN=1 (build de distribución): el .exe pide administrador al abrir.
// Sin esa variable (desarrollo): corre sin elevar y la app entra en modo solo lectura.
use embed_manifest::manifest::ExecutionLevel;
use embed_manifest::{embed_manifest, new_manifest};

fn main() {
    println!("cargo:rerun-if-env-changed=BX_ADMIN");
    let level = if std::env::var("BX_ADMIN").as_deref() == Ok("1") {
        ExecutionLevel::RequireAdministrator
    } else {
        ExecutionLevel::AsInvoker
    };
    embed_manifest(new_manifest("Botox.Optimizer").requested_execution_level(level)).expect("no se pudo incrustar el manifiesto");

    // Ícono del .exe (Explorador, escritorio, barra de tareas). windres viene con mingw64 (cargo.cmd lo pone en el PATH).
    let out = std::path::PathBuf::from(std::env::var("OUT_DIR").unwrap());
    let ico = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("assets").join("icon.ico");
    let rc = out.join("icon.rc");
    std::fs::write(&rc, format!("1 ICON \"{}\"\n", ico.display().to_string().replace('\\', "/"))).unwrap();
    let obj = out.join("icon.o");
    let ok = std::process::Command::new("windres").arg(&rc).arg("-O").arg("coff").arg("-o").arg(&obj).status().map_or(false, |s| s.success());
    assert!(ok, "windres falló: compilá con cargo.cmd para tener mingw64 en el PATH");
    println!("cargo:rustc-link-arg-bins={}", obj.display());
    println!("cargo:rerun-if-changed=assets/icon.ico");
    println!("cargo:rerun-if-changed=build.rs");
}
