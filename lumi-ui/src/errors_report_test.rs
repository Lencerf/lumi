use crate::errors_report::common_dir;

#[test]
fn test_common_dir() {
    assert_eq!(common_dir([].into_iter()), "");
    assert_eq!(
        common_dir(["/home/me/ledger/main.bean"].into_iter()),
        "/home/me/ledger/"
    );
    assert_eq!(
        common_dir(["/home/me/ledger/main.bean", "/home/me/ledger/2024/01.bean"].into_iter()),
        "/home/me/ledger/"
    );
    assert_eq!(
        common_dir(
            [
                "/home/me/ledger/2024/01.bean",
                "/home/me/ledger/2025/01.bean"
            ]
            .into_iter()
        ),
        "/home/me/ledger/"
    );
    // A shared name prefix is not a shared directory.
    assert_eq!(
        common_dir(["/home/me/ledger/a.bean", "/home/me/ledgers/b.bean"].into_iter()),
        "/home/me/"
    );
    assert_eq!(common_dir(["a.bean", "b.bean"].into_iter()), "");
    assert_eq!(common_dir(["/a.bean", "/b.bean"].into_iter()), "/");
}
