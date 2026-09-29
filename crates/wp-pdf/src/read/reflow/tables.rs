//! Tables with no rules between their columns.
//!
//! A table drawn with rules across it only — at its top and foot, and
//! perhaps under its heading row — is the lines between the rules, set in
//! columns. A table drawn with no rules at all is rows of short pieces of
//! text, each row cut at the same places: three rows at least, and pieces
//! of a few words, since columns of prose are a page's columns and not a
//! table's, and a list's markers are not a column either.

use super::super::content::Rectangle;
use super::{cut_at, Grid, Line, Ruled};

/// Rows: lines sharing a baseline, top first.
fn rows_of(lines: &[Line]) -> Vec<Vec<usize>> {
    let mut order: Vec<usize> = (0..lines.len()).collect();
    order.sort_by(|&a, &b| {
        lines[b]
            .y
            .partial_cmp(&lines[a].y)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(lines[a].x0.partial_cmp(&lines[b].x0).unwrap_or(std::cmp::Ordering::Equal))
    });
    let mut rows: Vec<Vec<usize>> = Vec::new();
    for index in order {
        match rows.last_mut() {
            Some(row) if (lines[row[0]].y - lines[index].y).abs() < 1.0 => row.push(index),
            _ => rows.push(vec![index]),
        }
    }
    rows
}

/// The columns the lines stand in: their spans across, joined where they
/// overlap, left to right.
fn columns_of(lines: &[&Line]) -> Vec<(f64, f64)> {
    let mut spans: Vec<(f64, f64)> = lines.iter().map(|l| (l.x0, l.x1)).collect();
    spans.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
    let mut columns: Vec<(f64, f64)> = Vec::new();
    for (x0, x1) in spans {
        match columns.last_mut() {
            Some(last) if x0 <= last.1 + 2.0 => last.1 = last.1.max(x1),
            _ => columns.push((x0, x1)),
        }
    }
    columns
}

/// A grid over rows of lines: the columns' edges halfway between them,
/// the rows' halfway between baselines — or at a rule, where one lies
/// between two rows.
fn grid_over(
    lines: &[Line],
    rows: &[Vec<usize>],
    columns: &[(f64, f64)],
    (left, right, top, bottom): (f64, f64, f64, f64),
    horizontals: Vec<Rectangle>,
    ruled: Ruled,
) -> Grid {
    let mut xs = vec![left];
    for pair in columns.windows(2) {
        xs.push((pair[0].1 + pair[1].0) / 2.0);
    }
    xs.push(right);
    let mut ys = vec![top];
    for pair in rows.windows(2) {
        let (upper, lower) = (&lines[pair[0][0]], &lines[pair[1][0]]);
        let between = horizontals
            .iter()
            .map(|h| (h.y0 + h.y1) / 2.0)
            .find(|&y| y < upper.y && y > lower.y + lower.size * 0.5);
        ys.push(between.unwrap_or((upper.y - upper.size * 0.25 + lower.top()) / 2.0));
    }
    ys.push(bottom);
    let (rows_count, columns_count) = (ys.len() - 1, xs.len() - 1);
    Grid {
        x0: left,
        y0: bottom,
        x1: right,
        y1: top,
        xs,
        ys,
        horizontals,
        verticals: Vec::new(),
        cells: vec![Vec::new(); rows_count * columns_count],
        ruled,
    }
}

/// The lines put into a grid's cells, row by row.
fn fill(grid: &mut Grid, lines: Vec<Line>, rows: &[Vec<usize>]) {
    let columns = grid.columns();
    let inner = grid.xs[1..grid.xs.len() - 1].to_vec();
    let mut row_of = vec![0; lines.len()];
    for (row, members) in rows.iter().enumerate() {
        for &index in members {
            row_of[index] = row;
        }
    }
    for (index, line) in lines.into_iter().enumerate() {
        let row = row_of[index];
        for part in cut_at(line, &inner) {
            let x = (part.x0 + part.x1) / 2.0;
            let column = grid.xs.windows(2).position(|w| x >= w[0] && x <= w[1]).unwrap_or(0);
            grid.cells[row * columns + column].push(part);
        }
    }
}

/// Tables between rules drawn across the page that start and end together
/// and are not a ruled grid's.
pub(super) fn ruled_across(
    rectangles: &[Rectangle],
    grids: &[Grid],
    lines: &mut Vec<Line>,
) -> Vec<Grid> {
    let rules: Vec<Rectangle> = rectangles
        .iter()
        .filter(|r| r.height() <= 2.5 && r.width() >= 36.0)
        .filter(|r| {
            let (x, y) = ((r.x0 + r.x1) / 2.0, (r.y0 + r.y1) / 2.0);
            !grids
                .iter()
                .any(|g| x >= g.x0 - 2.0 && x <= g.x1 + 2.0 && y >= g.y0 - 2.0 && y <= g.y1 + 2.0)
        })
        .copied()
        .collect();
    let mut sets: Vec<Vec<Rectangle>> = Vec::new();
    for rule in rules {
        match sets
            .iter_mut()
            .find(|set| (set[0].x0 - rule.x0).abs() <= 3.0 && (set[0].x1 - rule.x1).abs() <= 3.0)
        {
            Some(set) => set.push(rule),
            None => sets.push(vec![rule]),
        }
    }
    let mut out = Vec::new();
    for mut set in sets.into_iter().filter(|set| set.len() >= 2) {
        set.sort_by(|a, b| b.y0.partial_cmp(&a.y0).unwrap_or(std::cmp::Ordering::Equal));
        let top = (set[0].y0 + set[0].y1) / 2.0;
        let last = set.last().expect("two rules");
        let bottom = (last.y0 + last.y1) / 2.0;
        let (left, right) = (set[0].x0, set[0].x1);
        let (inside, outside): (Vec<Line>, Vec<Line>) =
            std::mem::take(lines).into_iter().partition(|line| {
                let x = (line.x0 + line.x1) / 2.0;
                x >= left && x <= right && line.y < top && line.y > bottom
            });
        *lines = outside;
        let rows = rows_of(&inside);
        let columns = columns_of(&inside.iter().collect::<Vec<_>>());
        if rows.len() < 2 || columns.len() < 2 {
            lines.extend(inside);
            continue;
        }
        let mut grid =
            grid_over(&inside, &rows, &columns, (left, right, top, bottom), set, Ruled::Across);
        fill(&mut grid, inside, &rows);
        out.push(grid);
    }
    out
}

/// Whether a piece of text is only a list's marker: a bullet, or a number
/// or letter with its stop.
fn is_marker(text: &str) -> bool {
    let text = text.trim();
    let mut chars = text.chars();
    match chars.next() {
        Some(
            '\u{2022}' | '\u{25CF}' | '\u{25E6}' | '\u{25AA}' | '\u{2013}' | '-' | '*' | '\u{B7}',
        ) => chars.next().is_none(),
        Some(_) => {
            let body = text.trim_end_matches(['.', ')']);
            body.len() < text.len()
                && body.len() <= 4
                && (body.chars().all(|c| c.is_ascii_digit())
                    || body.chars().all(char::is_alphabetic))
        }
        None => false,
    }
}

/// Tables with no rules: runs of three rows or more, each cut into two
/// pieces or more that stand in the same columns, the pieces short.
pub(super) fn aligned(lines: &mut Vec<Line>) -> Vec<Grid> {
    let rows = rows_of(lines);
    let mut runs: Vec<Vec<Vec<usize>>> = Vec::new();
    let mut current: Vec<Vec<usize>> = Vec::new();
    for row in rows {
        let close = current.last().is_some_and(|previous: &Vec<usize>| {
            let (above, here) = (&lines[previous[0]], &lines[row[0]]);
            above.y - here.y < 2.5 * above.size.max(here.size)
        });
        if row.len() >= 2 && (current.is_empty() || close) {
            current.push(row);
        } else {
            if current.len() >= 3 {
                runs.push(std::mem::take(&mut current));
            }
            current.clear();
            if row.len() >= 2 {
                current.push(row);
            }
        }
    }
    if current.len() >= 3 {
        runs.push(current);
    }
    let mut taken: Vec<usize> = Vec::new();
    let mut out = Vec::new();
    for rows in runs {
        let members: Vec<&Line> = rows.iter().flatten().map(|&i| &lines[i]).collect();
        let columns = columns_of(&members);
        if columns.len() < 2 {
            continue;
        }
        // Short pieces: a few words each.
        let texts: Vec<String> = members.iter().map(|line| line.text()).collect();
        let words = texts.iter().map(|t| t.split_whitespace().count()).sum::<usize>() as f64
            / texts.len() as f64;
        let characters =
            texts.iter().map(|t| t.chars().count()).sum::<usize>() as f64 / texts.len() as f64;
        if words > 4.0 || characters > 30.0 {
            continue;
        }
        // A list's markers beside its items are not a column.
        let first_column: Vec<&String> = members
            .iter()
            .zip(&texts)
            .filter(|(line, _)| line.x1 <= columns[0].1 + 0.5)
            .map(|(_, text)| text)
            .collect();
        if first_column.iter().all(|text| is_marker(text)) {
            continue;
        }
        let first = &lines[rows[0][0]];
        let last = &lines[rows[rows.len() - 1][0]];
        let left = columns[0].0 - 2.0;
        let right = columns[columns.len() - 1].1 + 2.0;
        let top = first.top() + 1.0;
        let bottom = last.y - last.size * 0.3;
        // The rows again, numbered among these lines only.
        let indices: Vec<usize> = rows.iter().flatten().copied().collect();
        let local: Vec<Line> = indices.iter().map(|&i| lines[i].clone()).collect();
        let local_rows = rows_of(&local);
        let mut grid = grid_over(
            &local,
            &local_rows,
            &columns,
            (left, right, top, bottom),
            Vec::new(),
            Ruled::Unruled,
        );
        fill(&mut grid, local, &local_rows);
        out.push(grid);
        taken.extend(indices);
    }
    taken.sort_unstable();
    for index in taken.into_iter().rev() {
        lines.remove(index);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_lists_markers_are_known() {
        assert!(is_marker("\u{2022}"));
        assert!(is_marker("12."));
        assert!(is_marker("b)"));
        assert!(!is_marker("Total"));
        assert!(!is_marker("3.50"));
    }
}
