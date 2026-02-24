use soroban_sdk::Env;

fn main() {
    let env = Env::default();
    let snapshot = env.to_ledger_snapshot();
    println!("Entries count: {}", snapshot.entries().count());
}
