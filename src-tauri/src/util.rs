pub fn human_bytes(n: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut v = n as f64;
    let mut u = 0;
    while v >= 1024.0 && u < UNITS.len() - 1 {
        v /= 1024.0;
        u += 1;
    }
    if u == 0 {
        format!("{n} B")
    } else {
        format!("{v:.1} {}", UNITS[u])
    }
}

pub fn unix_now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// File stem of a filename ("pump.svg" → "pump").
pub fn file_stem(filename: &str) -> &str {
    match filename.rfind('.') {
        Some(i) if i > 0 => &filename[..i],
        _ => filename,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn bytes_and_stems() {
        assert_eq!(human_bytes(512), "512 B");
        assert_eq!(human_bytes(1536), "1.5 KB");
        assert_eq!(human_bytes(25 * 1024 * 1024), "25.0 MB");
        assert_eq!(file_stem("a.b.svg"), "a.b");
        assert_eq!(file_stem(".hidden"), ".hidden");
    }
}
