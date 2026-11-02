use rust_decimal::Decimal;

use crate::number::{GroupedBuilder, GroupedText};

#[test]
fn test_groups_integer_part_every_three_digits_leaving_fraction_and_sign_intact() {
    let cases = [
        ("", "", 0),
        ("0", "0", 0),
        ("42", "42", 0),
        ("999.99", "999.99", 0),
        ("-123.45", "-123.45", 0),
        ("1000", "1 000", 1),
        ("-1000.00", "-1 000.00", 1),
        ("12345.67", "12 345.67", 1),
        ("-123456.78", "-123 456.78", 1),
        ("1123456.7891", "1 123 456.7891", 2),
        ("-1123456.789123", "-1 123 456.789123", 2),
        ("1000000000", "1 000 000 000", 3),
    ];

    for (input, expected, expected_breaks) in cases {
        let grouped = GroupedText::from_number_str(input);
        assert_eq!(grouped.as_str(), expected, "input: {input:?}");
        assert_eq!(grouped.ungrouped(), input, "input: {input:?}");
        assert_eq!(grouped.group_breaks(), expected_breaks, "input: {input:?}");
    }
}

#[test]
fn test_grouped_builder_preserves_dates_and_currency_spaces_between_amounts() {
    let mut builder = GroupedBuilder::new();
    builder.push_decimal("10000".parse::<Decimal>().unwrap());
    builder.push_str(" VTI {");
    builder.push_decimal("2500.00".parse::<Decimal>().unwrap());
    builder.push_str(" USD, 2024-03-01} @ ");
    builder.push_decimal("1200.50".parse::<Decimal>().unwrap());
    builder.push_str(" EUR");
    let text = builder.finish();

    assert_eq!(
        text.as_str(),
        "10 000 VTI {2 500.00 USD, 2024-03-01} @ 1 200.50 EUR"
    );
    assert_eq!(
        text.ungrouped(),
        "10000 VTI {2500.00 USD, 2024-03-01} @ 1200.50 EUR"
    );
    assert_eq!(text.group_breaks(), 3);
}
