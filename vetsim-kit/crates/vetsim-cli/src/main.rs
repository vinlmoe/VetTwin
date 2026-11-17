//! vetsim-cli : binaire de conformité (contrat : docs/CLI_CONTRACT.md).

fn main() {
    eprintln!(
        "vetsim-cli {} : aucune commande implémentée (M0)",
        vetsim_core::API_VERSION
    );
    std::process::exit(2);
}
