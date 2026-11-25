use super::ReleaseNote;

pub(crate) fn parse(markdown: &str) -> Vec<ReleaseNote> {
    markdown
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(|line| {
            let text = line
                .strip_prefix("## ")
                .or_else(|| line.strip_prefix("- "))
                .unwrap_or(line);
            ReleaseNote {
                new_entry: text.starts_with("✦ "),
                text: text
                    .strip_prefix("✦ ")
                    .unwrap_or(text)
                    .replace("**", "")
                    .into(),
                heading: line.starts_with("## "),
                bullet: line.starts_with("- "),
            }
        })
        .collect()
}

#[cfg(test)]
#[test]
fn release_021_retains_its_heading_icons_and_new_markers() {
    let notes = parse(include_str!("../release/notes/v0.2.1.md"));
    let headings: Vec<_> = notes
        .iter()
        .filter(|note| note.heading)
        .map(|note| note.text.as_str())
        .collect();
    assert_eq!(headings, ["🖥️ UI", "🌐 Net", "🛠️ For server owners"]);
    assert_eq!(notes.iter().filter(|note| note.new_entry).count(), 2);
}
