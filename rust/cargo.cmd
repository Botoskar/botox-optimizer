@echo off
rem Corre cargo con el Rust instalado en M:\Github Things\botox-optimizer-rust (no esta en el PATH del sistema).
rem Uso: cargo.cmd build --release   |   cargo.cmd run
rem
rem R: es una unidad virtual (subst) que apunta a esa carpeta. Hace falta porque dlltool y el gcc de MinGW
rem fallan con el espacio de "Github Things". No crea archivos fuera de la carpeta y se va al reiniciar.
if not exist R:\cargo subst R: "M:\Github Things\botox-optimizer-rust" >nul 2>&1
set "RUSTUP_HOME=R:\rustup"
set "CARGO_HOME=R:\cargo"
set "CARGO_TARGET_DIR=R:\target"
rem mingw64\bin (WinLibs) aporta dlltool, as, windres y el enlazador gcc que usa la variante GNU de Rust.
set "PATH=R:\cargo\bin;R:\mingw64\bin;%PATH%"
cd /d "%~dp0"
cargo %*
