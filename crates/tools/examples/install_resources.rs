//! Installs the real resources without a window, through the core, and prints their state:
//!
//!     cargo run -p greffier-tools --example install_resources [-- --check]
//!
//! The folder is the app's (`~/Library/Application Support/Greffier`). The bundled uv is taken
//! from `target/uv-<version>/uv`, where `cargo xtask install-app` puts it. `--check` only runs the
//! checks, without downloading anything.

use std::time::{Duration, Instant};

use greffier_core::adapters::{Adapters, ResourceState};
use greffier_tools::mac::{MacTools, UV_VERSION};
use greffier_core::resources::{Catalogue, AppResources, app_folder};
use greffier_core::{Command, Event, Greffier, demo};

fn main() {
    let uv = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(format!("../target/uv-{UV_VERSION}/uv"));
    let tools = if uv.is_file() { MacTools::with_uv(uv) } else { MacTools::new() };
    let folder = app_folder();
    let adapters = Adapters { resources: Box::new(AppResources::new(&folder, Catalogue::for_greffier(), tools)), ..demo::adapters() };
    let mut greffier = Greffier::open(std::env::temp_dir().join("greffier-install-resources"), adapters);
    let command = if std::env::args().any(|a| a == "--check") { Command::CheckResources } else { Command::InstallResources };
    greffier.execute(command).expect("command");
    println!("Folder: {}", folder.display());
    let start = Instant::now();
    let mut last = String::new();
    let mut failure = false;
    while greffier.busy() {
        std::thread::sleep(Duration::from_millis(500));
        for e in greffier.advance(0.5) {
            match e {
                Event::ResourcesInstalled => println!("Resources installed."),
                Event::InstallationFailed(m) => {
                    failure = true;
                    println!("Installation failed:\n{m}");
                }
                _ => {}
            }
        }
        let state = format!("{:?}", greffier.resources().iter().map(|r| (&r.name, &r.state)).collect::<Vec<_>>());
        if state != last && start.elapsed().as_secs().is_multiple_of(5) {
            println!("[{:>4} s] {}", start.elapsed().as_secs(), brief(&greffier));
            last = state;
        }
    }
    println!("\nDone after {} s:", start.elapsed().as_secs());
    for r in greffier.resources() {
        println!("  {:<22} {:?} · {} bytes · {}{}", r.name, r.state, r.size, r.detail, r.help.as_deref().map(|a| format!("\n      help: {a}")).unwrap_or_default());
    }
    std::process::exit(i32::from(failure));
}

fn brief(g: &Greffier) -> String {
    g.resources()
        .iter()
        .map(|r| match &r.state {
            ResourceState::Downloading { received, total } => format!("{} {:.0} %", r.name, *received as f64 * 100.0 / *total as f64),
            e => format!("{} {e:?}", r.name),
        })
        .collect::<Vec<_>>()
        .join(" | ")
}
