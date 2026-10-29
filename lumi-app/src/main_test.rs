use std::path::PathBuf;

use lumi_ui::Page;

use crate::{CliAction, parse_args};

fn parse(args: &[&str]) -> Result<CliAction, ()> {
    parse_args(args.iter().map(|arg| (*arg).to_string()))
}

#[test]
fn test_parse_args_allows_starting_without_a_root_file() {
    assert_eq!(
        parse(&[]),
        Ok(CliAction::Run {
            path: None,
            page: Page::BalanceSheet,
        })
    );
    assert_eq!(
        parse(&["--page", "journal"]),
        Ok(CliAction::Run {
            path: None,
            page: Page::Journal,
        })
    );
    assert_eq!(
        parse(&["--page=errors"]),
        Ok(CliAction::Run {
            path: None,
            page: Page::Errors,
        })
    );
}

#[test]
fn test_parse_args_accepts_a_root_file_and_page_in_either_order() {
    assert_eq!(
        parse(&["main.bean"]),
        Ok(CliAction::Run {
            path: Some(PathBuf::from("main.bean")),
            page: Page::BalanceSheet,
        })
    );
    assert_eq!(
        parse(&["main.bean", "--page", "income"]),
        Ok(CliAction::Run {
            path: Some(PathBuf::from("main.bean")),
            page: Page::Income,
        })
    );
    assert_eq!(
        parse(&["--page=account:Assets:Cash", "main.bean"]),
        Ok(CliAction::Run {
            path: Some(PathBuf::from("main.bean")),
            page: Page::Account("Assets:Cash".into()),
        })
    );
    assert_eq!(
        parse(&["--", "-weird.bean"]),
        Ok(CliAction::Run {
            path: Some(PathBuf::from("-weird.bean")),
            page: Page::BalanceSheet,
        })
    );
}

#[test]
fn test_parse_args_handles_help_and_rejects_bad_arguments() {
    assert_eq!(parse(&["-h"]), Ok(CliAction::Help));
    assert_eq!(parse(&["--help"]), Ok(CliAction::Help));
    assert_eq!(parse(&["--unknown"]), Err(()));
    assert_eq!(parse(&["--page"]), Err(()));
    assert_eq!(parse(&["--page", "bogus"]), Err(()));
    assert_eq!(parse(&["one.bean", "two.bean"]), Err(()));
}
