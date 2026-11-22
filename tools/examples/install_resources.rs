//! Installe les vraies ressources sans fenêtre, par le cœur, et affiche leur état :
//!
//!     cargo run -p greffier-outils --example installer_ressources [-- --verifier]
//!
//! Le dossier est celui de l'app (`~/Library/Application Support/Greffier`). L'uv embarqué est
//! pris dans `target/uv-<version>/uv`, où `scripts/installer-app` le dépose. `--verifier` ne fait
//! que les vérifications, sans rien télécharger.

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
    let command = if std::env::args().any(|a| a == "--verifier") { Command::CheckResources } else { Command::InstallResources };
    greffier.execute(command).expect("commande");
    println!("Dossier : {}", folder.display());
    let start = Instant::now();
    let mut last = String::new();
    let mut failure = false;
    while greffier.busy() {
        std::thread::sleep(Duration::from_millis(500));
        for e in greffier.advance(0.5) {
            match e {
                Event::ResourcesInstalled => println!("Ressources installées."),
                Event::InstallationFailed(m) => {
                    failure = true;
                    println!("Échec de l'installation :\n{m}");
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
    println!("\nFin après {} s :", start.elapsed().as_secs());
    for r in greffier.resources() {
        println!("  {:<22} {:?} · {} octets · {}{}", r.name, r.state, r.size, r.detail, r.help.as_deref().map(|a| format!("\n      aide : {a}")).unwrap_or_default());
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
