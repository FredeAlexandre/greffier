//! Les vrais adaptateurs de Greffier qui pilotent des programmes du Mac. Le cœur ne connaît que
//! leurs interfaces ([`greffier_coeur::adaptateurs`]) :
//!
//! - [`claude`] : l'[`Agent`](greffier_coeur::adaptateurs::Agent) de Greffier, la CLI `claude`
//!   (relecture en deux passes avec réécoute, bilan écrit par l'agent).
//! - [`mac`] : les [`Outils`](greffier_coeur::ressources::Outils) des ressources (curl, sha256,
//!   uv embarqué, vérification de `claude`) et l'environnement du Python privé.

pub mod claude;
pub mod mac;
