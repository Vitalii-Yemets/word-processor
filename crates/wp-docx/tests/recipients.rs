//! Sorting, filtering and checking a list of people a letter is going to.

use wp_docx::merge::Recipients;

/// A list with the awkward cases in it: two people the same, one with no
/// address, numbers that sort one way as words and another as numbers.
fn list() -> Recipients {
    Recipients::parse(
        b"Last Name,City,Order\n\
          Ogg,Lancre,9\n\
          Nitt,Ankh-Morpork,10\n\
          ogg,lancre,9\n\
          ,Ankh-Morpork,2\n",
    )
}

#[test]
fn a_list_is_read_as_the_people_in_it() {
    let list = list();
    assert_eq!(list.headers, vec!["Last Name", "City", "Order"]);
    assert_eq!(list.len(), 4);
    assert_eq!(list.value(0, "City").as_deref(), Some("Lancre"));
}

#[test]
fn sorting_by_words_puts_them_in_the_order_a_person_expects() {
    let list = list();
    let order = list.sorted_by("Last Name", true);
    // The empty one first, then Nitt, then the two Oggs in the order they
    // were: sorting does not mind about case, and does not shuffle equals.
    assert_eq!(order, vec![3, 1, 0, 2], "{order:?}");

    let backwards = list.sorted_by("Last Name", false);
    assert_eq!(backwards.first(), Some(&0).or(Some(&2)), "{backwards:?}");
    assert_eq!(backwards.last(), Some(&3), "the empty one is at the other end");
}

#[test]
fn a_column_of_numbers_sorts_as_numbers() {
    // The whole point: as words, 10 comes before 9.
    let list = list();
    let order = list.sorted_by("Order", true);
    let values: Vec<String> =
        order.iter().map(|at| list.value(*at, "Order").unwrap_or_default()).collect();
    assert_eq!(values, vec!["2", "9", "9", "10"], "{values:?}");
}

#[test]
fn filtering_finds_the_rows_that_hold_the_words() {
    let list = list();
    assert_eq!(list.matching("City", "lancre"), vec![0, 2], "case was minded");
    assert_eq!(list.matching("City", "ankh"), vec![1, 3], "part of a word was not enough");
    assert_eq!(list.matching("", "ogg"), vec![0, 2], "an empty column looks everywhere");
    assert_eq!(list.matching("City", "").len(), 4, "an empty answer leaves everybody in");
    assert!(list.matching("City", "Genua").is_empty());
}

#[test]
fn the_copies_are_found_and_the_originals_are_not() {
    let list = list();
    // Row 2 says what row 0 says, in another case and with spaces; what a
    // person wants from Find Duplicates is the copy, not the original.
    assert_eq!(list.duplicates(), vec![2]);
}

#[test]
fn a_row_with_nothing_to_send_to_is_named_as_such() {
    let list = list();
    assert!(list.missing_from(0).is_empty(), "a whole row was called short");
    let missing = list.missing_from(3);
    assert_eq!(missing, vec!["a name".to_owned()], "{missing:?}");

    // A list with no address column at all is short of an address in every
    // row, which is the honest answer.
    let names = Recipients::parse(b"Last Name\nOgg\n");
    assert_eq!(names.missing_from(0), vec!["an address".to_owned()]);
}

#[test]
fn an_email_counts_as_somewhere_to_send_it() {
    let list = Recipients::parse(b"Last Name,Email\nOgg,nanny@lancre.invalid\n");
    assert!(list.missing_from(0).is_empty(), "{:?}", list.missing_from(0));
}

#[test]
fn reordering_puts_the_rows_where_the_order_says() {
    let mut list = list();
    let order = list.sorted_by("Order", true);
    list.reorder(&order);
    let values: Vec<String> =
        (0..list.len()).map(|at| list.value(at, "Order").unwrap_or_default()).collect();
    assert_eq!(values, vec!["2", "9", "9", "10"]);
}

#[test]
fn somebody_typed_from_nothing_arrives_with_their_columns() {
    let mut list = Recipients::default();
    list.add(&[
        ("First Name".to_owned(), "Agnes".to_owned()),
        ("Last Name".to_owned(), "Nitt".to_owned()),
    ]);
    assert_eq!(list.headers, vec!["First Name", "Last Name"]);
    assert_eq!(list.len(), 1);

    // A second person with a column the first did not have adds the column.
    list.add(&[
        ("Last Name".to_owned(), "Ogg".to_owned()),
        ("City".to_owned(), "Lancre".to_owned()),
    ]);
    assert_eq!(list.headers, vec!["First Name", "Last Name", "City"]);
    assert_eq!(list.value(1, "City").as_deref(), Some("Lancre"));
    assert_eq!(list.value(1, "First Name").as_deref(), Some(""), "a gap is a gap");
    assert_eq!(list.value(0, "City").as_deref(), Some(""), "and the first person has one now");
}

#[test]
fn a_list_written_out_reads_back_as_itself() {
    let mut list = Recipients::default();
    list.add(&[
        ("Last Name".to_owned(), "Ogg, the elder".to_owned()),
        ("Note".to_owned(), "She said \"no\"".to_owned()),
    ]);
    let written = list.to_delimited();
    let read = Recipients::parse(written.as_bytes());

    assert_eq!(read.headers, list.headers);
    assert_eq!(read.value(0, "Last Name").as_deref(), Some("Ogg, the elder"), "{written}");
    assert_eq!(read.value(0, "Note").as_deref(), Some("She said \"no\""), "{written}");
}

#[test]
fn an_empty_list_writes_its_columns_and_nobody() {
    let list = Recipients { headers: vec!["Name".to_owned()], rows: Vec::new() };
    assert_eq!(list.to_delimited(), "Name\n");
}
