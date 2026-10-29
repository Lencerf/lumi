use std::path::PathBuf;

use lumi_ui::Page;

use crate::{CliAction, file_url_to_path, parse_args};

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
fn test_parse_args_ignores_macos_launch_services_psn_flag() {
    assert_eq!(
        parse(&["-psn_0_123456"]),
        Ok(CliAction::Run {
            path: None,
            page: Page::BalanceSheet,
        })
    );
    assert_eq!(
        parse(&["-psn_0_123456", "main.bean"]),
        Ok(CliAction::Run {
            path: Some(PathBuf::from("main.bean")),
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

#[test]
fn test_file_url_to_path_decodes_local_file_urls() {
    assert_eq!(
        file_url_to_path("file:///Users/alice/Ledgers/main.bean"),
        Some(PathBuf::from("/Users/alice/Ledgers/main.bean"))
    );
    assert_eq!(
        file_url_to_path(
            "file://localhost/Users/alice/My%20Ledgers/2026%20%E8%B4%A6%E6%9C%AC.bean"
        ),
        Some(PathBuf::from("/Users/alice/My Ledgers/2026 账本.bean"))
    );
    assert_eq!(
        file_url_to_path("file:///Users/alice/main.beancount?line=10#top"),
        Some(PathBuf::from("/Users/alice/main.beancount"))
    );
    assert_eq!(file_url_to_path("https://example.com/main.bean"), None);
    assert_eq!(file_url_to_path("file://remote-host/share/main.bean"), None);
    assert_eq!(file_url_to_path("file:///bad%ZZescape.bean"), None);
}
