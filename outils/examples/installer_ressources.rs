//! Installe les vraies ressources sans fenêtre, par le cœur, et affiche leur état :
//!
//!     cargo run -p greffier-outils --example installer_ressources [-- --verifier]
//!
//! Le dossier est celui de l'app (`~/Library/Application Support/Greffier`). L'uv embarqué est
//! pris dans `target/uv-<version>/uv`, où `scripts/installer-app` le dépose. `--verifier` ne fait
//! que les vérifications, sans rien télécharger.

use std::time::{Duration, Instant};

use greffier_coeur::adaptateurs::{Adaptateurs, EtatDeRessource};
use greffier_outils::mac::{OutilsDuMac, VERSION_D_UV};
use greffier_coeur::ressources::{Catalogue, RessourcesDeLApp, dossier_de_l_app};
use greffier_coeur::{Commande, Evenement, Greffier, fictif};

fn main() {
    let uv = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(format!("../target/uv-{VERSION_D_UV}/uv"));
    let outils = if uv.is_file() { OutilsDuMac::avec_uv(uv) } else { OutilsDuMac::new() };
    let dossier = dossier_de_l_app();
    let adaptateurs = Adaptateurs { ressources: Box::new(RessourcesDeLApp::new(&dossier, Catalogue::de_greffier(), outils)), ..fictif::adaptateurs() };
    let mut greffier = Greffier::ouvrir(std::env::temp_dir().join("greffier-installer-ressources"), adaptateurs);
    let commande = if std::env::args().any(|a| a == "--verifier") { Commande::VerifierRessources } else { Commande::InstallerRessources };
    greffier.executer(commande).expect("commande");
    println!("Dossier : {}", dossier.display());
    let debut = Instant::now();
    let mut dernier = String::new();
    let mut echec = false;
    while greffier.occupe() {
        std::thread::sleep(Duration::from_millis(500));
        for e in greffier.avancer(0.5) {
            match e {
                Evenement::RessourcesInstallees => println!("Ressources installées."),
                Evenement::InstallationEnEchec(m) => {
                    echec = true;
                    println!("Échec de l'installation :\n{m}");
                }
                _ => {}
            }
        }
        let etat = format!("{:?}", greffier.ressources().iter().map(|r| (&r.nom, &r.etat)).collect::<Vec<_>>());
        if etat != dernier && debut.elapsed().as_secs().is_multiple_of(5) {
            println!("[{:>4} s] {}", debut.elapsed().as_secs(), resume(&greffier));
            dernier = etat;
        }
    }
    println!("\nFin après {} s :", debut.elapsed().as_secs());
    for r in greffier.ressources() {
        println!("  {:<22} {:?} · {} octets · {}{}", r.nom, r.etat, r.taille, r.detail, r.aide.as_deref().map(|a| format!("\n      aide : {a}")).unwrap_or_default());
    }
    std::process::exit(i32::from(echec));
}

fn resume(g: &Greffier) -> String {
    g.ressources()
        .iter()
        .map(|r| match &r.etat {
            EtatDeRessource::EnTelechargement { recu, total } => format!("{} {:.0} %", r.nom, *recu as f64 * 100.0 / *total as f64),
            e => format!("{} {e:?}", r.nom),
        })
        .collect::<Vec<_>>()
        .join(" | ")
}
