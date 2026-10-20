//! Read the live input between Claude's last two rules. SGR is style metadata,
//! never executed. Unknown layouts and styles fail closed.
mod read;
pub(super) use read::screen;

#[derive(Default)]
struct Style {
    dim: bool,
    italic: bool,
    inverse: bool,
}

type Rows = Vec<Vec<(char, bool, bool)>>;

fn parse(screen: &str) -> Option<Rows> {
    if screen.len() > 128 * 1024 {
        return None;
    }
    let mut style = Style::default();
    let mut rows = vec![Vec::new()];
    let mut chars = screen.chars().peekable();
    while let Some(ch) = chars.next() {
        match ch {
            '\u{1b}' => {
                if chars.next() != Some('[') {
                    return None;
                }
                let mut parameters = String::new();
                loop {
                    match chars.next() {
                        Some('m') => break,
                        Some(c) if (c.is_ascii_digit() || c == ';') && parameters.len() < 128 => {
                            parameters.push(c)
                        }
                        _ => return None,
                    }
                }
                let codes: Vec<u16> = parameters
                    .split(';')
                    .map(|s| {
                        if s.is_empty() {
                            Some(0)
                        } else {
                            s.parse().ok()
                        }
                    })
                    .collect::<Option<_>>()?;
                let mut index = 0;
                while let Some(code) = codes.get(index) {
                    match code {
                        0 => style = Style::default(),
                        2 => style.dim = true,
                        22 => style.dim = false,
                        3 => style.italic = true,
                        23 => style.italic = false,
                        7 => style.inverse = true,
                        27 => style.inverse = false,
                        38 | 48 | 58 => {
                            index += match codes.get(index + 1) {
                                Some(5) => 2,
                                Some(2) => 4,
                                _ => return None,
                            };
                            codes.get(index)?;
                        }
                        _ => {}
                    }
                    index += 1;
                }
            }
            '\n' => rows.push(Vec::new()),
            '\r' => {}
            c if c.is_control() && c != '\t' => return None,
            c => {
                if let Some(row) = rows.last_mut() {
                    row.push((c, style.dim || style.italic, style.inverse));
                }
            }
        }
    }
    Some(rows)
}

fn plain(screen: &str) -> Option<String> {
    let lines: Vec<_> = parse(screen)?
        .iter()
        .map(|row| {
            row.iter()
                .map(|(c, _, _)| *c)
                .collect::<String>()
                .trim_end()
                .to_owned()
        })
        .collect();
    Some(lines.join("\n").trim_end().to_owned())
}

pub(super) fn empty(screen: &str) -> bool {
    let Some(rows) = parse(screen) else {
        return false;
    };
    let rule = |row: &Vec<(char, bool, bool)>| {
        row.iter().filter(|(c, _, _)| !c.is_whitespace()).count() >= 8
            && row
                .iter()
                .all(|(c, _, _)| matches!(c, '─' | '━') || c.is_whitespace())
    };
    let Some(bottom) = rows.iter().rposition(rule) else {
        return false;
    };
    let Some(top) = rows[..bottom].iter().rposition(rule) else {
        return false;
    };
    let mut content = rows[top + 1..bottom]
        .iter()
        .flatten()
        .filter(|(c, _, _)| !c.is_whitespace());
    if content.next().map(|(c, _, _)| *c) != Some('❯') {
        return false;
    }
    let body: Vec<_> = content.collect();
    if body.is_empty() {
        return true;
    }
    let text: String = body.iter().map(|(c, _, _)| *c).collect();
    if text.contains("[Pasted") || text.contains("[Image") {
        return false;
    }
    body.iter()
        .enumerate()
        .all(|(i, (_, dim, inverse))| *dim || (i == 0 && *inverse))
        && body.iter().any(|(_, dim, _)| *dim)
}

#[cfg(test)]
mod tests;
