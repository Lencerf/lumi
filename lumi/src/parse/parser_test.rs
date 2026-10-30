use std::fs;
use std::path::PathBuf;

use crate::{DEFAULT_DECIMAL_POINT_COLUMN, ErrorLevel, ErrorType, Ledger};

struct TempBean(PathBuf);

impl TempBean {
    fn new(name: &str, contents: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "lumi-parser-test-{}-{name}.bean",
            std::process::id()
        ));
        fs::write(&path, contents).unwrap();
        Self(path)
    }

    fn path(&self) -> &str {
        self.0.to_str().unwrap()
    }
}

impl Drop for TempBean {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

#[test]
fn test_plugin_and_custom_directives_parse_as_noop_info_diagnostics() {
    let ledger_src = "\
option \"operating_currency\" \"USD\"
plugin \"beancount.plugins.sellgains\"
plugin \"beancount.plugins.unrealized\" \"Unrealized\"

2020-01-01 custom \"fava-option\" \"auto-reload\" \"True\"
2020-01-01 custom \"budget\" Expenses:Food \"monthly\" 500.00 USD 2020-02-01 TRUE
  note: \"monthly cap\"

2020-01-01 open Assets:Cash
2020-01-01 open Expenses:Food

2020-01-02 * \"Coffee\"
  Expenses:Food   5.00 USD
  Assets:Cash    -5.00 USD
";
    let bean = TempBean::new("plugin-custom-info", ledger_src);
    let (ledger, diagnostics) = Ledger::from_file(bean.path());

    assert_eq!(ledger.txns().len(), 1);
    assert!(
        diagnostics.iter().all(|d| d.level == ErrorLevel::Info),
        "expected only Info diagnostics, got: {diagnostics:?}"
    );
    let summary: Vec<(ErrorLevel, ErrorType, usize, usize, &str)> = diagnostics
        .iter()
        .map(|d| {
            (
                d.level,
                d.r#type,
                d.src.start.line,
                d.src.start.col,
                d.msg.as_str(),
            )
        })
        .collect();
    assert_eq!(
        summary,
        vec![
            (
                ErrorLevel::Info,
                ErrorType::Unsupported,
                2,
                1,
                "Ignored directive: plugin \"beancount.plugins.sellgains\"."
            ),
            (
                ErrorLevel::Info,
                ErrorType::Unsupported,
                3,
                1,
                "Ignored directive: plugin \"beancount.plugins.unrealized\"."
            ),
            (
                ErrorLevel::Info,
                ErrorType::Unsupported,
                5,
                1,
                "Ignored directive: custom \"fava-option\"."
            ),
            (
                ErrorLevel::Info,
                ErrorType::Unsupported,
                6,
                1,
                "Ignored directive: custom \"budget\"."
            ),
        ]
    );
}

#[test]
fn test_malformed_plugin_and_custom_report_syntax_errors_and_recover() {
    let ledger_src = "\
plugin 123
plugin \"beancount.plugins.auto\"
2020-01-01 custom 456
2020-01-01 open Assets:Cash
";
    let bean = TempBean::new("plugin-custom-malformed", ledger_src);
    let (ledger, diagnostics) = Ledger::from_file(bean.path());

    assert!(ledger.accounts().contains_key(&"Assets:Cash".to_string()));
    let levels_and_lines: Vec<(ErrorLevel, ErrorType, usize)> = diagnostics
        .iter()
        .map(|d| (d.level, d.r#type, d.src.start.line))
        .collect();
    assert_eq!(
        levels_and_lines,
        vec![
            (ErrorLevel::Error, ErrorType::Syntax, 1),
            (ErrorLevel::Info, ErrorType::Unsupported, 2),
            (ErrorLevel::Error, ErrorType::Syntax, 3),
        ]
    );
}

#[test]
fn test_lumi_option_decimal_point_column_parses_without_diagnostics_and_validates() {
    let default_bean = TempBean::new("lumi-opt-default", "2020-01-01 open Assets:Cash\n");
    let (default_ledger, default_diags) = Ledger::from_file(default_bean.path());
    assert!(default_diags.is_empty(), "{default_diags:?}");
    assert_eq!(
        default_ledger.decimal_point_column(),
        DEFAULT_DECIMAL_POINT_COLUMN
    );

    let quoted_bean = TempBean::new(
        "lumi-opt-quoted",
        "1994-05-30 custom \"lumi-option\" \"decimal-point-column\" \"60\"\n2020-01-01 open Assets:Cash\n",
    );
    let (quoted_ledger, quoted_diags) = Ledger::from_file(quoted_bean.path());
    assert!(quoted_diags.is_empty(), "{quoted_diags:?}");
    assert_eq!(quoted_ledger.decimal_point_column(), 60);

    let numeric_bean = TempBean::new(
        "lumi-opt-number",
        "1994-05-30 custom \"lumi-option\" \"decimal-point-column\" 44\n2020-01-01 open Assets:Cash\n",
    );
    let (numeric_ledger, numeric_diags) = Ledger::from_file(numeric_bean.path());
    assert!(numeric_diags.is_empty(), "{numeric_diags:?}");
    assert_eq!(numeric_ledger.decimal_point_column(), 44);

    let invalid_bean = TempBean::new(
        "lumi-opt-invalid",
        "\
1994-05-30 custom \"lumi-option\" \"decimal-point-column\" \"0\"
1994-05-30 custom \"lumi-option\" \"decimal-point-column\" \"abc\"
1994-05-30 custom \"lumi-option\" \"unknown-key\" \"1\"
1994-05-30 custom \"lumi-option\" \"decimal-point-column\" \"58\"
1994-05-30 custom \"lumi-option\" \"decimal-point-column\" \"62\"
",
    );
    let (invalid_ledger, invalid_diags) = Ledger::from_file(invalid_bean.path());
    assert_eq!(invalid_ledger.decimal_point_column(), 58);
    let summary: Vec<(ErrorLevel, ErrorType, usize)> = invalid_diags
        .iter()
        .map(|d| (d.level, d.r#type, d.src.start.line))
        .collect();
    assert_eq!(
        summary,
        vec![
            (ErrorLevel::Error, ErrorType::Syntax, 1),
            (ErrorLevel::Error, ErrorType::Syntax, 2),
            (ErrorLevel::Warning, ErrorType::Unsupported, 3),
            (ErrorLevel::Warning, ErrorType::Duplicate, 5),
        ]
    );
}
