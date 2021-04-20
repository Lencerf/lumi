use clap::clap_app;
use lumi::Ledger;
use rust_decimal::prelude::Zero;

const VERSION: &str = env!("CARGO_PKG_VERSION");
const AUTHOR: &str = env!("CARGO_PKG_AUTHORS");

fn files(ledger: Ledger) {
    for file in ledger.files() {
        println!("{}", file);
    }
}

fn balances(ledger: Ledger) {
    let mut result = vec![];
    for (account, account_map) in ledger.balance_sheet() {
        if ledger.accounts()[account].close().is_some() {
            continue;
        }
        for (currency, currency_map) in account_map {
            for (cost, number) in currency_map {
                if number.is_zero() {
                    continue;
                }
                if let Some(cost) = cost {
                    result.push(format!("{} {} {} {}", account, number, currency, cost));
                } else {
                    result.push(format!("{} {} {}", account, number, currency));
                }
            }
        }
    }
    result.sort();
    for entry in result {
        println!("{}", entry);
    }
}
fn main() {
    let matches = clap_app!(lumi =>
        (version: VERSION)
        (author: AUTHOR)
        (@arg INPUT: +required "Input file")
        (@subcommand balances =>
            (about: "List the final balances of all accounts")
        )
        (@subcommand files =>
            (about: "List all source files")
        )

    )
    .get_matches();
    let path = matches.value_of("INPUT").unwrap();
    let (ledger, errors) = Ledger::from_file(path);
    for error in errors {
        println!("{}\n", error);
    }
    match matches.subcommand_name() {
        Some("balances") => balances(ledger),
        Some("files") => files(ledger),
        _ => {}
    }
}
