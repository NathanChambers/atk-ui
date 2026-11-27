use std::{
    env,
    fs::File,
    io::{self, Read},
};

// Keep this module consistent across the standalone ATK packages.
pub fn print_version() -> io::Result<()> {
    let path = env::current_exe()?;
    let mut executable = File::open(path)?;
    let mut build_id = 0xcbf2_9ce4_8422_2325_u64;
    let mut buffer = [0; 32 * 1024];
    loop {
        let bytes_read = executable.read(&mut buffer)?;
        if bytes_read == 0 {
            break;
        }

        for byte in &buffer[..bytes_read] {
            build_id ^= u64::from(*byte);
            build_id = build_id.wrapping_mul(0x0000_0100_0000_01b3);
        }
    }

    println!("{} {}", env!("CARGO_BIN_NAME"), env!("CARGO_PKG_VERSION"));
    println!("Build: fnv1a64:{build_id:016x}");
    Ok(())
}
