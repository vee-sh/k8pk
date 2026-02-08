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

    let cmd = cli::Cli::command();
    clap_mangen::generate_to(cmd, &out_dir).expect("failed to generate man pages");
}
