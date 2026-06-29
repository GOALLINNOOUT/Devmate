use anyhow::Result;
use comfy_table::{presets::ASCII_FULL, Cell, Table};
use serde::Serialize;

pub fn print_json<T: Serialize>(value: &T) -> Result<()> {
    anstream::println!("{}", serde_json::to_string_pretty(value)?);
    Ok(())
}

pub fn table(headers: &[&str], rows: Vec<Vec<String>>) -> Table {
    let mut table = Table::new();
    table.load_preset(ASCII_FULL);
    table.set_header(headers.iter().map(|header| Cell::new(*header)));
    for row in rows {
        table.add_row(row);
    }
    table
}

pub fn status(installed: bool) -> &'static str {
    if installed {
        "installed"
    } else {
        "missing"
    }
}

pub fn bytes(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{} {}", bytes, UNITS[unit])
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn table_uses_ascii_borders() {
        let rendered = table(
            &["Tool", "Status"],
            vec![vec!["Rust".to_string(), "installed".to_string()]],
        )
        .to_string();

        assert!(rendered.contains('+'));
        assert!(!rendered.contains('\u{250c}'));
    }
}
