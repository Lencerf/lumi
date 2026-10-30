use crate::align::{adjusted_cursor_for_alignment, diff_summary};

#[test]
fn test_diff_summary_identifies_prefix_suffix_and_space_only_deletion() {
    let prev = "    Liabilities:WellsFargo:Bilt                -2.36 USD";
    let cur = "    Liabilities:WellsFargo:Bilt               -2.36 USD";
    let (prefix, suffix, only_deleted_spaces) = diff_summary(prev, cur);
    assert!(only_deleted_spaces);
    assert_eq!(prev.len() - prefix - suffix, 1);
    assert_eq!(cur.len() - prefix - suffix, 0);

    let (_, _, non_space_delete) = diff_summary(
        "    Liabilities:WellsFargo:Bilt                -2.36 USD",
        "    Liabilities:WellsFargo:Bil                -2.36 USD",
    );
    assert!(!non_space_delete);

    let (_, _, space_insert) = diff_summary(
        "    Liabilities:WellsFargo:Bilt -2.36 USD",
        "    Liabilities:WellsFargo:Bilt  -2.36 USD",
    );
    assert!(!space_insert);
}

#[test]
fn test_adjusted_cursor_for_alignment_preserves_caret_in_account_whitespace_and_number() {
    // Editing inside or at the end of the account keeps the caret untouched.
    assert_eq!(adjusted_cursor_for_alignment(31, 31..32, 16, false), 31);
    assert_eq!(adjusted_cursor_for_alignment(15, 31..47, 17, false), 15);

    // Editing inside or after the number shifts by `desired_spaces - old_spaces`.
    assert_eq!(adjusted_cursor_for_alignment(34, 31..32, 16, false), 49);
    assert_eq!(adjusted_cursor_for_alignment(50, 31..47, 15, false), 49);

    // Backspacing a space immediately before the number keeps the caret moving
    // left rather than snapping back to the number's start.
    assert_eq!(adjusted_cursor_for_alignment(46, 31..46, 16, true), 46);

    // Deleting a non-space character at the start of the number (e.g. `-12` -> `12`)
    // keeps the caret at the number's new start.
    assert_eq!(adjusted_cursor_for_alignment(46, 31..46, 17, false), 48);
}
