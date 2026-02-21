use clap::CommandFactory;
use std::fs;
use std::io;
use std::path::Path;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

// Mock crate root modules that src/cli/args.rs depends on
#[allow(dead_code)]
mod config {
    pub struct Config {
        pub debug: DebugConfig,
        pub output: OutputConfig,
    }

    pub struct DebugConfig {
        pub breakpoints: Vec<String>,
        pub verbosity: Option<u8>,
    }

    pub struct OutputConfig {
        pub format: Option<String>,
        pub show_events: Option<bool>,
    }
}

#[allow(dead_code)]
#[path = "src/cli/args.rs"]
mod args;

use args::Cli;

fn main() -> std::io::Result<()> {
    emit_build_metadata();
    generate_man_pages()?;

    println!("cargo:rerun-if-changed=.git/HEAD");
    println!("cargo:rerun-if-changed=src/cli/args.rs");
    println!("cargo:rerun-if-changed=build.rs");

    Ok(())
}

fn emit_build_metadata() {
    let git_hash = command_stdout("git", &["rev-parse", "--short", "HEAD"])
        .unwrap_or_else(|| "unknown".to_string());
    let rustc_version =
        command_stdout("rustc", &["--version"]).unwrap_or_else(|| "unknown".to_string());
    let build_date = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()
        .map(|d| d.as_secs())
        .unwrap_or(0);

    println!("cargo:rustc-env=GIT_HASH={}", git_hash);
    println!("cargo:rustc-env=RUSTC_VERSION={}", rustc_version);
    println!("cargo:rustc-env=BUILD_DATE={}", build_date);
}

fn command_stdout(program: &str, args: &[&str]) -> Option<String> {
    Command::new(program)
        .args(args)
        .output()
        .ok()
        .and_then(|output| String::from_utf8(output.stdout).ok())
        .map(|s| s.trim().to_string())
}

fn generate_man_pages() -> std::io::Result<()> {
    let cmd = Cli::command();
    let repo_man_dir = Path::new("man").join("man1");

    match render_to_dir(&cmd, &repo_man_dir) {
        Ok(()) => Ok(()),
        Err(err) if err.kind() == io::ErrorKind::PermissionDenied => {
            let out_dir = std::env::var("OUT_DIR").unwrap_or_else(|_| "target".to_string());
            let fallback_dir = Path::new(&out_dir).join("man1");
            println!(
                "cargo:warning=Cannot write man pages to {} (permission denied). Writing to {} instead.",
                repo_man_dir.display(),
                fallback_dir.display()
            );
            render_to_dir(&cmd, &fallback_dir)
        }
        Err(err) => Err(err),
    }
}

fn render_to_dir(cmd: &clap::Command, dir: &Path) -> std::io::Result<()> {
    fs::create_dir_all(dir)?;
    render_recursive(cmd, dir, "")
}

fn render_recursive(cmd: &clap::Command, out_dir: &Path, prefix: &str) -> std::io::Result<()> {
    let name = if prefix.is_empty() {
        cmd.get_name().to_string()
    } else {
        format!("{}-{}", prefix, cmd.get_name())
    };

    let cmd = cmd.clone().name(name.clone());
    let man = clap_mangen::Man::new(cmd.clone());
    let mut buffer: Vec<u8> = Default::default();
    man.render(&mut buffer)?;
    fs::write(out_dir.join(format!("{}.1", name)), buffer)?;

    for sub in cmd.get_subcommands() {
        if !sub.is_hide_set() {
            render_recursive(sub, out_dir, &name)?;
        }
    }

    Ok(())
}
