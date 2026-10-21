use std::fs;

fn main() -> anyhow::Result<()> {
    let elf_path = std::env::args()
        .nth(1)
        .expect("Usage: package_elf <elf_path> <out_path>");
    let out_path = std::env::args()
        .nth(2)
        .expect("Usage: package_elf <elf_path> <out_path>");
    let bytecode = fs::read(&elf_path)?;
    let program_bytes =
        risc0_binfmt::ProgramBinary::new(&bytecode, risc0_zkos_v1compat::V1COMPAT_ELF).encode();
    fs::write(&out_path, program_bytes)?;
    println!(
        "Packaged {} -> {} ({} bytes)",
        elf_path,
        out_path,
        fs::metadata(&out_path)?.len()
    );
    Ok(())
}
