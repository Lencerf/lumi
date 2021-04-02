use clap::{clap_app, ArgMatches};
use lumi::Ledger;
use rust_decimal::prelude::Zero;

const VERSION: &str = env!("CARGO_PKG_VERSION");
const AUTHOR: &str = env!("CARGO_PKG_AUTHORS");

fn balances(matches: &ArgMatches) {
    let path = matches.value_of("INPUT").unwrap();
    let (ledger, errors) = Ledger::from_file(path);

    for error in errors {
        println!("{}\n", error);
    }

    let mut result = vec![];
    for (account, account_map) in ledger.balance_sheet() {
        if ledger.accounts[account].close.is_some() {
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
        (@subcommand balances =>
            (@arg INPUT: +required "Input file")
        )
    )
    .get_matches();
    if let Some(matches) = matches.subcommand_matches("balances") {
        balances(&matches);
    }
}
