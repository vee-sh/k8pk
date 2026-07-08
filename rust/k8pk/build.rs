use clap::CommandFactory;
use std::fs;
use std::path::PathBuf;

#[path = "src/cli.rs"]
mod cli;

fn main() {
    // Only generate man pages when explicitly requested (e.g., during release)
    let out_dir = match std::env::var_os("K8PK_MAN_DIR") {
        Some(dir) => PathBuf::from(dir),
        None => return, // Skip man page generation during normal builds
    };

    fs::create_dir_all(&out_dir).expect("failed to create man page output directory");

    // clap_mangen renders the command tree recursively; on Windows the build
    // script's main thread only gets ~1 MB of stack, which overflows for our
    // deep subcommand/help tree. Run generation on a thread with a large stack.
    std::thread::Builder::new()
        .stack_size(16 * 1024 * 1024)
        .spawn(move || {
            let cmd = cli::Cli::command();
            clap_mangen::generate_to(cmd, &out_dir).expect("failed to generate man pages");
        })
        .expect("failed to spawn man page generation thread")
        .join()
        .expect("man page generation thread panicked");
}
